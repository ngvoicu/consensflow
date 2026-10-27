// What install/real-install.mjs refuses before it reads anything: run outside GitHub Actions on a machine that
// has a ConsensFlow in use, it would replace that one, and with --upgrade or --downgrade open it
// and then end it (a Mac's processes are asked to stop; Windows' are stopped, the daemon too). A Mac
// must name the folder it installs into, and Windows the installed ConsensFlow it replaces (--dir,
// which must be the folder Windows lists it in), and both must have --no-open for those two.
//
// The script is run as it is, started by `node --import` of a preload that stands in for what the
// machine does not give a test: on "Windows" a `process.platform` of win32 and a `powershell` that
// answers as the uninstall entry does, and on both a `fetch` that records what it was asked and
// answers "not found", so a run that gets past its guard ends at the first thing it reads, and
// nothing goes to GitHub.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import { ROOT } from '../install/kit.mjs'

const SCRIPT = path.join(ROOT, 'install/real-install.mjs')
const FOLDER = 'C:\\Users\\dev\\AppData\\Local\\ConsensFlow'

let work
let fetched

before(() => {
  work = mkdtempSync(path.join(os.tmpdir(), 'cf-real-install-guard-'))
  fetched = path.join(work, 'fetched')
  writeFileSync(
    path.join(work, 'windows.mjs'),
    "Object.defineProperty(process, 'platform', { value: 'win32' })\n",
  )
  writeFileSync(
    path.join(work, 'fetch.mjs'),
    `import { appendFileSync } from 'node:fs'
globalThis.fetch = async (url) => {
  appendFileSync(${JSON.stringify(fetched)}, \`\${url}\\n\`)
  return { ok: false }
}
`,
  )
})

after(() => {
  if (work) rmSync(work, { recursive: true, force: true })
})

/**
 * Runs the script with `args`. `windows` is the uninstall entry a faked Windows lists
 * (`{ version, folder }`, or null for none), `ci` whether CI=true is set, `actions` whether GITHUB_ACTIONS is; not given, the machine's own.
 */
function realInstall(args, { windows, ci = false, actions = false } = {}) {
  rmSync(fetched, { force: true })
  const bin = path.join(work, 'bin')
  mkdirSync(bin, { recursive: true })
  writeFileSync(
    path.join(bin, 'powershell'),
    `#!/bin/sh\n${windows ? `echo ${windows.version}\necho '${windows.folder}'\n` : ''}`,
    { mode: 0o755 },
  )
  const env = {
    PATH: `${bin}:${process.env.PATH}`,
    HOME: work,
    ...(ci ? { CI: 'true' } : {}),
    ...(actions ? { GITHUB_ACTIONS: 'true', CI: 'true' } : {}),
  }
  const imports = [...(windows === undefined ? [] : ['windows.mjs']), 'fetch.mjs']
  const ran = spawnSync(
    process.execPath,
    [...imports.flatMap((file) => ['--import', path.join(work, file)]), SCRIPT, ...args],
    { encoding: 'utf8', env },
  )
  let asked = []
  try {
    asked = readFileSync(fetched, 'utf8').split('\n').filter(Boolean)
  } catch {
    // nothing was asked
  }
  return { status: ran.status, out: `${ran.stdout}${ran.stderr}`, asked }
}

const installed = { version: '3.0.7-alpha.9', folder: FOLDER }

describe('on Windows, outside GitHub Actions', {
  skip: process.platform === 'win32' && 'it fakes Windows',
}, () => {
  test('an installed ConsensFlow is not replaced, and nothing is read, unless the run names it', () => {
    const ran = realInstall(['--channel', 'alpha'], { windows: installed })
    assert.equal(ran.status, 1, ran.out)
    assert.match(
      ran.out,
      /ConsensFlow 3\.0\.7-alpha\.9 is installed in C:\\Users\\dev\\AppData\\Local\\ConsensFlow[\s\S]*--dir/,
    )
    assert.deepEqual(ran.asked, [], 'nothing was read from GitHub')
  })

  test('naming another folder than the installed one is not naming it', () => {
    const ran = realInstall(['--dir', 'D:\\somewhere\\else'], { windows: installed })
    assert.equal(ran.status, 1, ran.out)
    assert.match(ran.out, /is installed in C:\\Users\\dev\\AppData\\Local\\ConsensFlow/)
    assert.deepEqual(ran.asked, [])
  })

  test('naming the installed one, however it is spelled, goes on', () => {
    for (const spelled of [
      FOLDER,
      `${FOLDER}\\`,
      FOLDER.toLowerCase(),
      FOLDER.replaceAll('\\', '/'),
    ]) {
      const ran = realInstall(['--dir', spelled], { windows: installed })
      assert.equal(ran.asked.length, 1, `${spelled}: ${ran.out}`)
      assert.match(ran.asked[0], /feed-alpha\/latest\.json$/)
    }
  })

  test('--upgrade and --downgrade end the installed app, so they need --no-open even where it is named', () => {
    for (const flag of ['--upgrade', '--downgrade']) {
      const ran = realInstall(['--dir', FOLDER, flag], { windows: installed })
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, /--no-open/)
      assert.deepEqual(ran.asked, [], flag)
      const allowed = realInstall(['--dir', FOLDER, flag, '--no-open'], { windows: installed })
      assert.equal(allowed.asked.length, 1, `${flag} --no-open: ${allowed.out}`)
    }
  })

  test('with no ConsensFlow installed there is none to replace', () => {
    const ran = realInstall(['--channel', 'alpha'], { windows: null })
    assert.equal(ran.asked.length, 1, ran.out)
  })

  test("on GitHub's runner, which is the run's own, nothing is asked of it", () => {
    const ran = realInstall(['--upgrade'], { windows: installed, actions: true })
    assert.equal(ran.asked.length, 1, ran.out)
  })

  test('CI=true, which many tools and developers set, is not a runner of GitHub: the guard holds', () => {
    const ran = realInstall(['--downgrade'], { windows: installed, ci: true })
    assert.equal(ran.status, 1, ran.out)
    assert.match(ran.out, /is installed in C:\\Users\\dev\\AppData\\Local\\ConsensFlow/)
    assert.deepEqual(ran.asked, [])
  })
})

describe('on a Mac, outside GitHub Actions', {
  skip: process.platform !== 'darwin' && 'it is a Mac',
}, () => {
  test('the folder to install into is named, and --upgrade and --downgrade run with --no-open', () => {
    const none = realInstall([])
    assert.equal(none.status, 1, none.out)
    assert.match(none.out, /name the folder to install into with --dir/)
    assert.deepEqual(none.asked, [])
    for (const flag of ['--upgrade', '--downgrade']) {
      const ran = realInstall(['--dir', path.join(work, 'apps'), flag])
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, /--no-open/)
      assert.deepEqual(ran.asked, [], flag)
    }
    const named = realInstall(['--dir', path.join(work, 'apps'), '--upgrade', '--no-open'])
    assert.equal(named.asked.length, 1, named.out)
    // CI=true is no GitHub runner: the folder is still to be named.
    const ci = realInstall([], { ci: true })
    assert.equal(ci.status, 1, ci.out)
    assert.match(ci.out, /name the folder to install into with --dir/)
    assert.deepEqual(ci.asked, [])
  })
})
