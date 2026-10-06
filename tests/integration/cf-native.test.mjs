import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { stageBundle } from '../bundle.mjs'
import { chooseHome } from '../choice.mjs'
import { outsideAWindow } from '../core-api-fixture.mjs'

/**
 * A command `cf` hands to the CLI beside it, `cf.mjs` (a home that has taken the
 * way back to Node, the `use-node` file in it, hands it every command), goes to
 * the Node the bundle carries, which `cf` finds from its own place in the
 * bundle, with its arguments as they came. The Windows `.cmd` this replaced ran
 * through cmd.exe, which ends a command at its first line break: a reviewer's
 * 6,250-character question reached the chief as its first line, 528 characters,
 * and `cf` said it was asked (2026-10-03). The native `cf` npm run build:cf built
 * runs here in a bundle laid out as the app's, beside a `cf.mjs` that says what
 * it was given, outside any window: a window's token makes `cf` the board.
 */

const WINDOWS = process.platform === 'win32'
const BUILT = fileURLToPath(new URL(`../../bin/${WINDOWS ? 'cf.exe' : 'cf'}`, import.meta.url))
/** Line breaks, quotes, a variable cmd.exe would expand, its operators, diacritics. */
const TEXT = [
  'Întrebarea 1: verific și versiunea în engleză?',
  'He said "only the Romanian one" & left; 100% sure | %PATH% ^ !x!',
  '',
  'PLOP-6142',
].join('\n')

let dir
let home
let bundle
let cf
before(() => {
  assert.ok(existsSync(BUILT), `missing built cf: ${BUILT}; build it with npm run build:cf`)
  dir = mkdtempSync(join(tmpdir(), 'cf-native-'))
  home = join(dir, 'consensflow')
  // A home that has taken the way back to Node.
  chooseHome('node', home)
  bundle = stageBundle({ sources: false })
  cf = bundle.cf
  // What it was given, in ASCII, so no console's code page bends it on the
  // way back; `-` reads stdin, and `fail` exits 3.
  writeFileSync(
    bundle.cfMjs,
    `import { readFileSync } from 'node:fs'
const args = process.argv.slice(2)
const given = { args, stdin: args.includes('-') ? readFileSync(0, 'utf8') : null }
process.stdout.write(JSON.stringify(given).replace(/[\\u0080-\\uffff]/g, (c) => '\\\\u' + c.charCodeAt(0).toString(16).padStart(4, '0')))
process.exitCode = args[0] === 'fail' ? 3 : 0
`,
  )
})
after(() => {
  bundle.cleanup()
  rmSync(dir, { recursive: true, force: true })
})

/** The home is the test's own, with the way back's file in it; no Node is named. */
const run = (file, args, options = {}) =>
  spawnSync(file, args, {
    encoding: 'utf8',
    env: { ...outsideAWindow(), CONSENSFLOW_HOME: home },
    ...options,
  })

describe("cf's commands outside a window", () => {
  it('gives the CLI every argument whole: line breaks, quotes, % and & and ^, diacritics', () => {
    const ran = run(cf, ['ask', TEXT])
    assert.equal(ran.status, 0, ran.stderr)
    assert.deepEqual(JSON.parse(ran.stdout), { args: ['ask', TEXT], stdin: null })
  })

  it('hands its stdin on to the CLI, and the CLI’s exit code back', () => {
    const piped = run(cf, ['ask', '-'], { input: TEXT })
    assert.deepEqual(JSON.parse(piped.stdout), { args: ['ask', '-'], stdin: TEXT })
    assert.equal(run(cf, ['fail']).status, 3)
  })

  it('runs on the Node of its own bundle, whichever Node an environment names', () => {
    const named = run(cf, ['ask', 'x'], {
      env: {
        ...outsideAWindow(),
        CONSENSFLOW_HOME: home,
        CONSENSFLOW_NODE: join(dir, 'no-such-node'),
      },
    })
    assert.equal(named.status, 0, named.stderr)
    assert.deepEqual(JSON.parse(named.stdout), { args: ['ask', 'x'], stdin: null })
  })

  it('says so when its bundle has no Node, and how to be rid of the file', () => {
    const bare = stageBundle({ sources: false, node: false })
    try {
      writeFileSync(bare.cfMjs, '')
      const ran = run(bare.cf, ['ask', 'x'])
      assert.equal(ran.status, 1)
      assert.equal(ran.stdout, '')
      assert.match(
        ran.stderr,
        /sends this home's commands to Node, and none is bundled beside this cf \(looked for /,
      )
      assert.match(ran.stderr, /delete the file to run the native cf/)
    } finally {
      bare.cleanup()
    }
  })

  // Codex runs its commands in PowerShell, and Claude Code has a PowerShell
  // tool: both ran `& '…\cf.cmd' ask $q` with $q read from a file.
  it('takes a many-line argument whole from PowerShell', { skip: !WINDOWS }, () => {
    // Windows PowerShell 5.1 gives a native command an argument's own double
    // quotes bare, so this one has none.
    const text = TEXT.replace(/"/g, '')
    writeFileSync(join(dir, 'question.md'), text)
    const ran = run('powershell.exe', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `$q = Get-Content -Raw -Encoding UTF8 '${join(dir, 'question.md')}'; & '${cf}' ask $q`,
    ])
    assert.equal(ran.status, 0, ran.stderr)
    assert.deepEqual(JSON.parse(ran.stdout), { args: ['ask', text], stdin: null })
  })
})
