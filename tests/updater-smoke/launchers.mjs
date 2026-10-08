import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { copyFileSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { terminalEnv } from './box.mjs'
import { RELEASES } from './build.mjs'

/**
 * The terminal's command (`cf` and `consensflow` in the `bin` of a ConsensFlow
 * home), as an installed app's `cf setup` writes it and as the app that
 * replaces it repairs it at its start. The installed app is the bridge, whose
 * `cf setup` is Node's and writes a command that names the bundled Node and
 * `cf.mjs`; or the flip release, whose `cf setup` is the native `cf`'s and
 * writes a command that names the `cf` of its bundle, the very path the update's
 * `cf` is at. What the repair does of them, and what it does not, is the
 * contract (`cf_launcher::repair`, app/src-tauri/src/launcher.rs):
 *
 * - a command that serves the app's own home and does not run the bundle's
 *   `cf` is rewritten to run it, keeping the home it pins: both names of it;
 * - one that already runs it is left as it is, and is not spoken of;
 * - one that serves another home is left as it is, byte for byte, whether it sits
 *   in this home's `bin` or in the other home's own.
 */

const MARKER = 'Installed by ConsensFlow'
const NAMES = ['cf', 'consensflow']

const binOf = (home) => join(home, 'bin')
const fileOf = (home, name) => join(binOf(home), name)

/** Runs the installed app's own `cf setup` (Node's in the bridge, the native `cf`'s in the flip) as a terminal does, on `home`. */
function setUp(installed, box, home, release) {
  const options = { env: terminalEnv(box, home), cwd: box.probe, stdio: ['ignore', 'pipe', 'pipe'] }
  if (RELEASES[release].setup === 'native') {
    execFileSync(installed.cf, ['setup'], options)
    return
  }
  execFileSync(
    join(installed.app, 'Contents', 'MacOS', 'node'),
    [join(installed.app, 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs'), 'setup'],
    options,
  )
}

/** What a terminal's command `name` of `home` says when it is asked for its version. */
export function versionOf(box, home, name = 'cf') {
  return execFileSync(fileOf(home, name), ['--version'], {
    env: terminalEnv(box, home),
    cwd: box.probe,
    encoding: 'utf8',
  }).trim()
}

/** The text of each command of `home`, by name. */
export function commandsOf(home) {
  return Object.fromEntries(NAMES.map((name) => [name, readFileSync(fileOf(home, name), 'utf8')]))
}

/**
 * The commands the app that is installed leaves on a machine: this home's, and
 * another home's (set up as the user of a second copy would). With `elsewhere`,
 * one name of this home's is replaced by the other home's command, which serves
 * another home from where this home's app looks; `repaired` names the commands
 * that serve this home, which the app that replaces this one is to look at. Each
 * is what the installed release's `cf setup` wrote: it runs the bundled Node and
 * its `cf.mjs` (the bridge's), or the bundle's native `cf` (the flip's).
 */
export function plantCommands(installed, box, { elsewhere, release }) {
  setUp(installed, box, box.state, release)
  setUp(installed, box, box.other, release)
  if (elsewhere) copyFileSync(fileOf(box.other, 'consensflow'), fileOf(box.state, 'consensflow'))
  const planted = {
    own: commandsOf(box.state),
    other: commandsOf(box.other),
    repaired: elsewhere ? ['cf'] : NAMES,
  }
  const node = join(installed.app, 'Contents', 'MacOS', 'node')
  const entry = join(installed.app, 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs')
  const runs =
    RELEASES[release].setup === 'native'
      ? { line: `exec "${installed.cf}" "$@"`, what: "the installed app's cf" }
      : { line: `exec "${node}" "${entry}" "$@"`, what: "the installed app's Node and cf.mjs" }
  const served = [
    ...planted.repaired.map((name) => [box.state, name, planted.own[name]]),
    ...NAMES.map((name) => [box.other, name, planted.other[name]]),
  ]
  for (const [home, name, text] of served) {
    const file = fileOf(home, name)
    assert.ok(text.includes(MARKER), `${file} is not ours`)
    assert.ok(text.includes(runs.line), `${file} does not run ${runs.what}:\n${text}`)
    assert.ok(text.includes(`export CONSENSFLOW_HOME="${home}"`), `${file} pins no ${home}`)
    assert.match(versionOf(box, home, name), /^\d+\.\d+\.\d+/, `${file} does not run`)
  }
  // What this home's bin holds that serves the other home: pinned to it.
  for (const name of NAMES.filter((each) => !planted.repaired.includes(each))) {
    const text = planted.own[name]
    assert.ok(text.includes(`export CONSENSFLOW_HOME="${box.other}"`), `${name} pins no other home`)
  }
  return planted
}

/**
 * The commands once the app that replaced the installed one has started: each
 * that serves this home runs the bundle's `cf` and still pins this home, and it
 * runs and says the version of the `cf` the daemon is. A command the bridge's
 * Node `cf setup` wrote was rewritten, and the app's log says so. One the flip's
 * `cf setup` wrote named the `cf` of the installed bundle, which is the path the
 * update's is at: it was current, and is as it was, byte for byte, and the log
 * says nothing of it. Nothing that serves another home is changed by a byte, or
 * is spoken of.
 */
export function assertRepaired({ box, planted, cf, appLogText, version, release }) {
  const now = commandsOf(box.state)
  for (const name of planted.repaired) {
    const file = fileOf(box.state, name)
    assert.ok(now[name].includes(MARKER), `${file} lost its mark`)
    assert.ok(now[name].includes(`exec "${cf}" "$@"`), `${file} does not run ${cf}:\n${now[name]}`)
    assert.ok(now[name].includes(`export CONSENSFLOW_HOME="${box.state}"`), `${file} lost the pin`)
    assert.ok(!/cf\.mjs|MacOS\/node/.test(now[name]), `${file} still names Node's:\n${now[name]}`)
    assert.ok(statSync(file).mode & 0o111, `${file} is not executable`)
    if (RELEASES[release].setup === 'native') {
      assert.deepEqual(now[name], planted.own[name], `${file}, which was current, changed`)
      assert.ok(!appLogText.includes(file), `app.log speaks of a command that was current: ${file}`)
    } else {
      assert.ok(
        appLogText.split('\n').some((line) => line.endsWith(`${file} now runs ${cf}`)),
        `app.log does not say ${file} was repaired:\n${appLogText.slice(-2000)}`,
      )
    }
    assert.equal(
      versionOf(box, box.state, name),
      version,
      `${file} runs another cf than the daemon`,
    )
  }
  for (const name of NAMES.filter((each) => !planted.repaired.includes(each))) {
    const file = fileOf(box.state, name)
    assert.equal(now[name], planted.own[name], `${file}, which serves another home, changed`)
    assert.ok(
      !appLogText.includes(file),
      `app.log speaks of a command that serves another home: ${file}`,
    )
  }
  assert.deepEqual(commandsOf(box.other), planted.other, "another home's commands changed")
  assert.ok(
    !appLogText.includes(binOf(box.other)),
    `app.log speaks of a command that serves another home: ${binOf(box.other)}`,
  )
}
