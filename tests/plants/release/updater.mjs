/**
 * Plants in what the updater smoke (`npm run smoke:updater`) reads as proof: the
 * app's process, its daemon's
 * process and readiness, the ledger's one holder, the ledger's contents, the
 * terminal command, the bundles it inspects and makes, the key and the versions
 * it builds with. Each takes one check out, and a test that needs no app
 * (tests/updater-smoke-*.test.mjs) must fail.
 *
 * The ones in the product are in updater-product.mjs.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { EVIDENCE, LAUNCHERS, SMOKE_DIR, SMOKE_KIT } from './kit.mjs'
import { PLANTS as PRODUCT } from './updater-product.mjs'

const EVIDENCE_JS = `${SMOKE_DIR}/evidence.mjs`
const LEDGER_JS = `${SMOKE_DIR}/ledger.mjs`
const LAUNCHERS_JS = `${SMOKE_DIR}/launchers.mjs`
const BUNDLE_JS = `${SMOKE_DIR}/bundle.mjs`
const BUILD_JS = `${SMOKE_DIR}/build.mjs`
const SIGNING_JS = `${SMOKE_DIR}/signing.mjs`
const VERSIONS_JS = `${SMOKE_DIR}/versions.mjs`
const PROCESSES_JS = `${SMOKE_DIR}/processes.mjs`

const kit = (name, file, from, to, run, meant) => ({
  name: `updater evidence: ${name}`,
  edits: [[file, from, to]],
  runs: [run],
  meant,
})

/** What the readers of the process table and the daemon's log must refuse. */
const DAEMON = [
  kit(
    'a daemon that is not the app’s child is taken for its daemon',
    EVIDENCE_JS,
    'const mine = rows.filter((row) => row.ppid === app)',
    'const mine = rows',
    EVIDENCE,
    'refuses a daemon that is not the app’s child',
  ),
  kit(
    'a home with a second daemon running is taken for one with a holder',
    EVIDENCE_JS,
    '    rows.length,\n    1,\n',
    '    rows.length,\n    rows.length,\n',
    EVIDENCE,
    'refuses a home with a second daemon running',
  ),
  kit(
    'a daemon that logged no start line is taken for a ready one',
    EVIDENCE_JS,
    'assert.notEqual(start, null, ',
    'assert.notEqual(start, 0, ',
    EVIDENCE,
    'refuses a daemon that never logged its start',
  ),
  kit(
    'a log that says one daemon where the process is the other is taken',
    EVIDENCE_JS,
    '    kindOfCommand(row.command),\n    start.kind,\n',
    '    start.kind,\n    start.kind,\n',
    EVIDENCE,
    'refuses a log that says one daemon',
  ),
  kit(
    'the errors a daemon logged are not looked at',
    EVIDENCE_JS,
    'assert.deepEqual(errorLines(written), [], ',
    'assert.deepEqual([], [], ',
    EVIDENCE,
    'refuses a daemon that logged an error',
  ),
  kit(
    'a daemon that failed to start (exit 1) is not looked for',
    EVIDENCE_JS,
    '!written.some((line) => / info exit 1$/.test(line)),',
    '!written.some((line) => / info never$/.test(line)),',
    EVIDENCE,
    'refuses an earlier daemon of the app that was refused its start',
  ),
  kit(
    'a daemon of the app that was refused the ledger is let through at the end',
    EVIDENCE_JS,
    '    refused,\n    probes.size,\n',
    '    refused,\n    refused,\n',
    EVIDENCE,
    'say so when a daemon the app started was refused the ledger',
  ),
  kit(
    'a probe’s refusal counts against the app’s daemons',
    EVIDENCE_JS,
    'startsOf(log).filter((each) => !probes.includes(each))',
    'startsOf(log)',
    EVIDENCE,
    'refuses an earlier daemon of the app that was refused its start',
  ),
  kit(
    'a process that is not the app’s executable is taken for the app',
    EVIDENCE_JS,
    'assert.equal(row.command, binary, ',
    'assert.equal(binary, binary, ',
    EVIDENCE,
    'takes the app for the process that is the bundle’s executable',
  ),
  kit(
    'a zombie is taken for the running app',
    EVIDENCE_JS,
    "row !== undefined && !row.state.startsWith('Z')",
    'row !== undefined',
    EVIDENCE,
    'takes the app for the process that is the bundle’s executable',
  ),
  kit(
    'an app log that does not name the native daemon is taken for one that does',
    EVIDENCE_JS,
    'line.endsWith(said)',
    "line.includes('native')",
    EVIDENCE,
    'does not take Node’s way back, another home’s choice, or silence',
  ),
]

/** What the reader of the ledger’s one holder must refuse. */
const HOLDER = [
  kit(
    'a second ConsensFlow that ran is taken for one that was refused',
    EVIDENCE_JS,
    '    attempt.code,\n    1,\n',
    '    attempt.code,\n    attempt.code,\n',
    EVIDENCE,
    'is no proof when the second one ran',
  ),
  kit(
    'the ledger’s words are not looked for in the refusal',
    EVIDENCE_JS,
    'assert.ok(\n    words.test(attempt.err),',
    'assert.ok(\n    true,',
    EVIDENCE,
    'is no proof when the second one ran',
  ),
  kit(
    'a probe that never ended is taken for a refused one',
    EVIDENCE_JS,
    'assert.equal(attempt.signal, null, ',
    'assert.equal(null, null, ',
    EVIDENCE,
    'is no proof when the second one ran',
  ),
  kit(
    'a handle line from the second ConsensFlow is let through',
    EVIDENCE_JS,
    "assert.equal(attempt.out, '', ",
    "assert.equal('', '', ",
    EVIDENCE,
    'is no proof when the second one ran',
  ),
]

/** What the reader of the ledger’s contents must refuse. */
const CONTENTS = [
  kit(
    'a column that changed is let through',
    LEDGER_JS,
    'if (REWRITTEN.has(column)) continue',
    'continue',
    EVIDENCE,
    'is no longer whole when a row is gone',
  ),
  kit(
    'a row that is gone is let through',
    LEDGER_JS,
    'assert.ok(now !== undefined, ',
    'assert.ok(true, ',
    EVIDENCE,
    'is no longer whole when a row is gone',
  ),
  kit(
    'a table that is gone is let through',
    LEDGER_JS,
    'assert.ok(table in after.tables, ',
    'assert.ok(true, ',
    EVIDENCE,
    'is no longer whole when a row is gone',
  ),
  kit(
    'a schema that went back is let through',
    LEDGER_JS,
    'assert.ok(ledger.version >= atLeast, ',
    'assert.ok(true, ',
    EVIDENCE,
    'is not sound when its schema is below',
  ),
  kit(
    'SQLite’s own check of the file is not asked',
    LEDGER_JS,
    "assert.deepEqual(ledger.integrity, ['ok'], ",
    "assert.deepEqual(['ok'], ['ok'], ",
    EVIDENCE,
    'is not sound when its schema is below',
  ),
  kit(
    'a reference that does not hold is let through',
    LEDGER_JS,
    'assert.deepEqual(ledger.references, [], ',
    'assert.deepEqual([], [], ',
    EVIDENCE,
    'is not sound when its schema is below',
  ),
  kit(
    'a project that is not in the ledger is let through',
    LEDGER_JS,
    'assert.ok(held.includes(directory), ',
    'assert.ok(true, ',
    EVIDENCE,
    'has a project for each directory asked for',
  ),
  kit(
    'an event the daemon traced and the ledger lost is let through',
    LEDGER_JS,
    'assert.ok(held.includes(key), ',
    'assert.ok(true, ',
    EVIDENCE,
    'reads the events a daemon traced',
  ),
]

/** What the reader of the terminal command must refuse. */
const COMMANDS = [
  kit(
    'no command is looked at: all are taken for repaired',
    LAUNCHERS_JS,
    'for (const name of planted.repaired) {',
    'for (const name of []) {',
    LAUNCHERS,
    'is not repaired while it still names Node’s',
  ),
  kit(
    'only the first name is looked at: the second is taken for repaired',
    LAUNCHERS_JS,
    'for (const name of planted.repaired) {',
    'for (const name of planted.repaired.slice(0, 1)) {',
    LAUNCHERS,
    'is not repaired when the second name was left as it was',
  ),
  kit(
    'a command that does not run the update’s cf is taken for repaired',
    LAUNCHERS_JS,
    'assert.ok(now[name].includes(`exec "${cf}" "$@"`), ',
    'assert.ok(true, ',
    LAUNCHERS,
    'is not repaired while it still names Node’s',
  ),
  kit(
    'a repaired command that lost its pin is taken for repaired',
    LAUNCHERS_JS,
    'assert.ok(now[name].includes(`export CONSENSFLOW_HOME="${box.state}"`), ',
    'assert.ok(true, ',
    LAUNCHERS,
    'is not repaired while it still names Node’s',
  ),
  kit(
    'a repaired command that lost its mark is taken for repaired',
    LAUNCHERS_JS,
    'assert.ok(now[name].includes(MARKER), ',
    'assert.ok(true, ',
    LAUNCHERS,
    'is not repaired while it still names Node’s',
  ),
  kit(
    'a repaired command that still names Node’s is taken for repaired',
    LAUNCHERS_JS,
    'assert.ok(!/cf\\.mjs|MacOS\\/node/.test(now[name]), ',
    'assert.ok(true, ',
    LAUNCHERS,
    'is not repaired while it still names Node’s',
  ),
  kit(
    'a repaired command that cannot be run is taken for repaired',
    LAUNCHERS_JS,
    'assert.ok(statSync(file).mode & 0o111, ',
    'assert.ok(true, ',
    LAUNCHERS,
    'is not repaired while it still names Node’s',
  ),
  kit(
    'a command that runs another cf than the daemon is taken for repaired',
    LAUNCHERS_JS,
    '      versionOf(box, box.state, name),\n      version,\n',
    '      version,\n      version,\n',
    LAUNCHERS,
    'is no repair when it runs another cf',
  ),
  kit(
    'an app log that does not say the repair is taken for one that does',
    LAUNCHERS_JS,
    '.some((line) => line.endsWith(`${file} now runs ${cf}`)),',
    '.some(() => true),',
    LAUNCHERS,
    'is not own-home-only',
  ),
  kit(
    'a command that serves another home, changed in this home’s bin, is let through',
    LAUNCHERS_JS,
    'assert.equal(now[name], planted.own[name], ',
    'assert.equal(now[name], now[name], ',
    LAUNCHERS,
    'is not own-home-only',
  ),
  kit(
    'an app log that speaks of the command pinned to another home is let through',
    LAUNCHERS_JS,
    '      !appLogText.includes(file),\n',
    '      true,\n',
    LAUNCHERS,
    'is not own-home-only',
  ),
  kit(
    'the commands of another home, changed, are let through',
    LAUNCHERS_JS,
    'assert.deepEqual(commandsOf(box.other), planted.other, ',
    'assert.deepEqual(commandsOf(box.other), commandsOf(box.other), ',
    LAUNCHERS,
    'is not own-home-only',
  ),
  kit(
    'an app log that speaks of another home’s command is let through',
    LAUNCHERS_JS,
    '    !appLogText.includes(binOf(box.other)),\n',
    '    true,\n',
    LAUNCHERS,
    'is not own-home-only',
  ),
]

/** What the inspection of a bundle, the refused bundles it makes, and what a build is given, must hold to. */
const BUNDLES = [
  kit(
    'a process that is under the app but not its child is not looked for',
    PROCESSES_JS,
    'if (row.ppid === parent && !found.includes(row.pid)) {',
    'if (row.ppid === root && !found.includes(row.pid)) {',
    SMOKE_KIT,
    'finds everything under a process',
  ),
  kit(
    'a tree is not ended, whatever the app made of it',
    PROCESSES_JS,
    '    if (under.length === 0 && !alive(root)) return\n',
    '    return\n',
    SMOKE_KIT,
    'ends a process and all that is under it',
  ),
  kit(
    'a bundle of another app is taken',
    BUNDLE_JS,
    'assert.equal(identifier, IDENTITY, ',
    'assert.equal(IDENTITY, IDENTITY, ',
    SMOKE_KIT,
    'refuses another app, two versions in one plist',
  ),
  kit(
    'a plist with two versions is taken',
    BUNDLE_JS,
    "    plistValue(app, 'CFBundleVersion'),\n    version,\n",
    '    version,\n    version,\n',
    SMOKE_KIT,
    'refuses another app, two versions in one plist',
  ),
  kit(
    'a bundle with no cf is taken',
    BUNDLE_JS,
    `    ["window's cf", cf],\n`,
    '',
    SMOKE_KIT,
    'refuses another app, two versions in one plist',
  ),
  kit(
    'a bundle with half of Node’s files is taken',
    BUNDLE_JS,
    'has.every(Boolean) || !has.some(Boolean),',
    'true,',
    SMOKE_KIT,
    'refuses another app, two versions in one plist',
  ),
  kit(
    'a bundle signed again is not verified: any seal passes',
    BUNDLE_JS,
    "  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app])",
    '  void app',
    SMOKE_KIT,
    'makes the refused bundles the way the smoke needs them',
  ),
  kit(
    'the bundle with no cf is not signed again, so the seal refuses it before the rule does',
    BUNDLE_JS,
    "      run('/usr/bin/codesign', ['--force', '--deep', '--sign', '-', app])\n",
    '      void app\n',
    SMOKE_KIT,
    'makes the refused bundles the way the smoke needs them',
  ),
  kit(
    'the tampered bundle is not tampered with',
    BUNDLE_JS,
    "      appendFileSync(\n        join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf'),\n        'changed after signing',\n      )\n",
    '      void app\n',
    SMOKE_KIT,
    'makes the refused bundles the way the smoke needs them',
  ),
  kit(
    'a replacement leaves the old files where they were',
    BUNDLE_JS,
    '  rmSync(to, { recursive: true, force: true })\n  return copyOver(from, to)',
    '  return copyOver(from, to)',
    SMOKE_KIT,
    'copies a bundle whole, replacing what was there',
  ),
  kit(
    'a build that does not carry the run’s key is taken',
    BUILD_JS,
    '  if (!holds(publicKey)) throw new Error(',
    '  if (false) throw new Error(',
    SMOKE_KIT,
    'is held to its key',
  ),
  kit(
    'a build that carries the product’s key is taken',
    BUILD_JS,
    '  if (holds(productKey)) throw new Error(',
    '  if (false) throw new Error(',
    SMOKE_KIT,
    'is held to its key',
  ),
  kit(
    'the update is not given its version',
    BUILD_JS,
    '  if (version !== undefined) config.version = version\n',
    '',
    SMOKE_KIT,
    'is the override of the run’s public key',
  ),
  kit(
    'a build inherits the signing variables of the shell it runs in',
    SIGNING_JS,
    'const SIGNING_VARIABLES = /^(TAURI_|APPLE_|CSC_)/',
    'const SIGNING_VARIABLES = /^(NOTHING_)/',
    SMOKE_KIT,
    'inherits no updater key, Apple certificate or identity',
  ),
  kit(
    'a build may use the network',
    SIGNING_JS,
    "env.CARGO_NET_OFFLINE = 'true'",
    "env.CARGO_NET_OFFLINE = 'false'",
    SMOKE_KIT,
    'inherits no updater key, Apple certificate or identity',
  ),
  kit(
    'the update is given the checkout’s version, which is the installed app’s',
    VERSIONS_JS,
    'compareVersions(checkout, latest) > 0 ? checkout : nextVersion(latest)',
    'checkout',
    SMOKE_KIT,
    'is the next one after the newest installed app’s',
  ),
  kit(
    'the update is given a version newer than the first installed app’s alone',
    VERSIONS_JS,
    'const latest = newest(installed)',
    'const latest = installed[0]',
    SMOKE_KIT,
    'is the next one after the newest installed app’s',
  ),
  kit(
    'the flip is the oldest release after the bridge',
    VERSIONS_JS,
    'compareVersions(tag.slice(1), latest.slice(1)) > 0 ? tag : latest,',
    'compareVersions(tag.slice(1), latest.slice(1)) < 0 ? tag : latest,',
    SMOKE_KIT,
    'has the flip release the newest tag after the bridge’s, and none where there is none',
  ),
  kit(
    'the release under test is taken for the flip',
    `${SMOKE_DIR}/build.mjs`,
    "return git('tag', '--merged', 'HEAD', '--list', 'v*').filter((tag) => !here.has(tag))",
    "return git('tag', '--merged', 'HEAD', '--list', 'v*')",
    SMOKE_KIT,
    'have the flip among the tags of this checkout’s history, but for the one under test',
  ),
  kit(
    'the flip’s terminal command is repaired though it was current',
    LAUNCHERS_JS,
    "    if (RELEASES[release].setup === 'native') {\n      assert.deepEqual(",
    '    if (false) {\n      assert.deepEqual(',
    LAUNCHERS,
    'is current: it names the cf of the bundle, which the update’s is at, and is as it was',
  ),
  kit(
    'the app that started is not held to the bundle’s cf',
    EVIDENCE_JS,
    'const said = `starting the daemon: ${cf} ui --json --no-open`',
    'const said = `starting the daemon: `',
    EVIDENCE,
    'does not take the flip’s sentence, another bundle’s cf, or silence',
  ),
]

export const PLANTS = [...DAEMON, ...HOLDER, ...CONTENTS, ...COMMANDS, ...BUNDLES, ...PRODUCT]
