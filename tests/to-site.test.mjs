// install/to-site.mjs: copies the installers to the site, and --check says whether the site holds
// the repository's two copies, byte for byte. A site that serves an installer older than the
// repository's is a release that installs without what the tests and the plants hold (the site
// branch once held round 3's install.sh when the repository's had the sweep of what a killed
// install leaves: T-840). The scripts are run as they are, against a site of the test's own.

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

/** A site folder of its own whose page shows `shows`, holding the installers named in `holds`. */
function site({ shows = KEY, holds = [] } = {}) {
  const folder = mkdtempSync(path.join(work, 'site-'))
  writeFileSync(path.join(folder, 'index.html'), `<p>The update key is ${shows}</p>\n`)
  for (const script of holds) copyFileSync(script, path.join(folder, path.basename(script)))
  return folder
}

function toSite(...args) {
  const ran = spawnSync(process.execPath, [TO_SITE, ...args], { encoding: 'utf8' })
  return { status: ran.status, out: `${ran.stdout}${ran.stderr}` }
}

describe('npm run site:installers', () => {
  test("copies both installers to the site, which then holds the repository's", () => {
    const folder = site()
    const ran = toSite(folder)
    assert.equal(ran.status, 0, ran.out)
    for (const script of SCRIPTS)
      assert.deepEqual(
        readFileSync(path.join(folder, path.basename(script))),
        readFileSync(script),
        path.basename(script),
      )
    assert.equal(toSite('--check', folder).status, 0)
  })

  test('is refused while the site shows another key than the installers pin, or none', () => {
    for (const shows of ['RWSomeOtherKeyThatIsNotTheOneTheInstallersPin00000000', 'no key']) {
      const folder = site({ shows })
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
  test('is satisfied by a site that holds the two installers byte for byte', () => {
    const ran = toSite('--check', site({ holds: SCRIPTS }))
    assert.equal(ran.status, 0, ran.out)
    assert.match(ran.out, /the site holds this repository's installers/)
  })

  test("refuses a site whose installer is not the repository's, saying which and how to mend it", () => {
    for (const script of SCRIPTS) {
      const folder = site({ holds: SCRIPTS })
      const at = path.join(folder, path.basename(script))
      // The earlier version of a script: the same but for one line.
      writeFileSync(at, readFileSync(script, 'utf8').replace(/\n/, '\n# an earlier version\n'))
      const ran = toSite('--check', folder)
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, new RegExp(`${path.basename(script).replace('.', '\\.')} differs`))
      assert.match(ran.out, /npm run site:installers/)
      for (const other of SCRIPTS.filter((one) => one !== script))
        assert.doesNotMatch(
          ran.out,
          new RegExp(`${path.basename(other).replace('.', '\\.')} differs`),
        )
    }
  })

  test('refuses a site that holds one of them, or none, and changes nothing', () => {
    for (const holds of [[INSTALL_SH], [INSTALL_PS1], []]) {
      const folder = site({ holds })
      const ran = toSite('--check', folder)
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, /is missing from the site|differs/)
      for (const script of SCRIPTS.filter((one) => !holds.includes(one)))
        assert.ok(!existsSync(path.join(folder, path.basename(script))), 'nothing was copied')
    }
  })

  test('refuses while the site shows another key than the installers pin, as the copy does', () => {
    const ran = toSite('--check', site({ shows: 'no key', holds: SCRIPTS }))
    assert.equal(ran.status, 1, ran.out)
    assert.match(ran.out, /does not show the key the installers pin/)
  })
})

// The site the installers are served from lies beside the main checkout of this repository, and
// holds the installers once its branch is on it: from then on the gate there holds it to them.
describe("the site beside this repository's folder", () => {
  const beside = path.join(ROOT, '..', 'sites', 'consensflow-site')
  const holdsThem = SCRIPTS.some((script) => existsSync(path.join(beside, path.basename(script))))
  test("holds the repository's installers, once it holds any", {
    skip: !holdsThem && 'the site holds no installers here',
  }, () => {
    const ran = toSite('--check', beside)
    assert.equal(ran.status, 0, ran.out)
  })
})
