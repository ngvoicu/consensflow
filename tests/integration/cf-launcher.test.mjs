import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * A pane's `cf` on Windows, bin/cf.exe (app/cf-launcher): ConsensFlow's CLI
 * on the app's runtime, with its arguments as they came. The `.cmd` it
 * replaced ran through cmd.exe, which ends a command at its first line
 * break: a reviewer's 6,250-character question reached the chief as its
 * first line, 528 characters, and `cf` said it was asked (2026-10-03). The
 * launcher is built here as Windows' app build builds it, and runs beside a
 * `cf.mjs` that says what it was given.
 */

const WINDOWS = process.platform === 'win32'
const CRATE = fileURLToPath(new URL('../../app/cf-launcher/', import.meta.url))
const BUILT = join(CRATE, 'target', 'release', WINDOWS ? 'cf.exe' : 'cf')
/** Line breaks, quotes, a variable cmd.exe would expand, its operators, diacritics. */
const TEXT = [
  'Întrebarea 1: verific și versiunea în engleză?',
  'He said "only the Romanian one" & left; 100% sure | %PATH% ^ !x!',
  '',
  'PLOP-6142',
].join('\n')

let dir
let cf
before(() => {
  execFileSync(
    'cargo',
    ['build', '--release', '--locked', '--manifest-path', join(CRATE, 'Cargo.toml')],
    { stdio: 'inherit' },
  )
  dir = mkdtempSync(join(tmpdir(), 'cf-launcher-'))
  cf = join(dir, WINDOWS ? 'cf.exe' : 'cf')
  copyFileSync(BUILT, cf)
  // What it was given, in ASCII, so no console's code page bends it on the
  // way back; `-` reads stdin, and `fail` exits 3.
  writeFileSync(
    join(dir, 'cf.mjs'),
    `import { readFileSync } from 'node:fs'
const args = process.argv.slice(2)
const given = { args, stdin: args.includes('-') ? readFileSync(0, 'utf8') : null }
process.stdout.write(JSON.stringify(given).replace(/[\\u0080-\\uffff]/g, (c) => '\\\\u' + c.charCodeAt(0).toString(16).padStart(4, '0')))
process.exitCode = args[0] === 'fail' ? 3 : 0
`,
  )
})
after(() => rmSync(dir, { recursive: true, force: true }))

const run = (file, args, options = {}) =>
  spawnSync(file, args, {
    encoding: 'utf8',
    env: { ...process.env, CONSENSFLOW_NODE: process.execPath },
    ...options,
  })

describe("a pane's cf.exe", () => {
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

  it('refuses to run on any Node but the one the app names', () => {
    const { CONSENSFLOW_NODE, ...env } = process.env
    const ran = run(cf, ['ask', 'x'], { env })
    assert.equal(ran.status, 1)
    assert.match(ran.stderr, /CONSENSFLOW_NODE is not set/)
    assert.equal(ran.stdout, '')
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
