import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * A verb of the native `cf` is given its arguments whole. The Windows `.cmd`
 * this replaced ran through cmd.exe, which ends a command at its first line
 * break: a reviewer's 6,250-character question reached the chief as its first
 * line, 528 characters, and `cf` said it was asked (2026-10-03). The native `cf`
 * `npm run build:cf` built runs here outside any window (a window's token makes
 * `cf` the board), and `cf agent add --description` keeps the text it is given
 * in the roster's file, so the roster is what shows what the verb was given.
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
before(() => {
  assert.ok(existsSync(BUILT), `missing built cf: ${BUILT}; build it with npm run build:cf`)
  dir = mkdtempSync(join(tmpdir(), 'cf-native-'))
  home = join(dir, 'consensflow')
})
after(() => {
  // Windows holds a just-ended program's files for a moment: removal retries (EPERM, 2026-10-08).
  rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 })
})

/** This process's environment without anything of a window it may itself run in, and the test's home. */
const outside = () => ({
  ...Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !/^(CONSENSFLOW_|CF_|CHISEL_)/.test(name)),
  ),
  CONSENSFLOW_HOME: home,
})

const run = (file, args, options = {}) =>
  spawnSync(file, args, { encoding: 'utf8', env: outside(), ...options })

/** The description the roster keeps for the agent called `name`. */
const described = (name) =>
  JSON.parse(readFileSync(join(home, 'agents.json'), 'utf8')).agents.find((row) => row.id === name)
    ?.description

describe("cf's verbs outside a window", () => {
  it('are given every argument whole: line breaks, quotes, % and & and ^, diacritics', () => {
    const ran = run(BUILT, [
      ...['agent', 'add', 'whole', '--harness', 'claude', '--model', 'a-model'],
      ...['--description', TEXT],
    ])
    assert.equal(ran.status, 0, ran.stderr)
    assert.equal(ran.stdout, 'whole  claude  a-model\n')
    assert.equal(described('whole'), TEXT)
  })

  it('end as the verb ends: its words on stderr, and its exit code', () => {
    const ran = run(BUILT, ['agent', 'add', 'lonely'])
    assert.equal(ran.status, 1)
    assert.equal(ran.stdout, '')
    assert.match(ran.stderr, /^cf: lonely needs --harness and --model/)
  })

  // Codex runs its commands in PowerShell, and Claude Code has a PowerShell
  // tool: both ran `& '…\cf.cmd' ask $q` with $q read from a file.
  it('take a many-line argument whole from PowerShell', { skip: !WINDOWS }, () => {
    // Windows PowerShell 5.1 gives a native command an argument's own double
    // quotes bare, so this one has none.
    const text = TEXT.replace(/"/g, '')
    writeFileSync(join(dir, 'question.md'), text)
    const ran = run('powershell.exe', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `$q = Get-Content -Raw -Encoding UTF8 '${join(dir, 'question.md')}'; & '${BUILT}' agent add shell --harness claude --model a-model --description $q`,
    ])
    assert.equal(ran.status, 0, ran.stderr)
    assert.equal(described('shell'), text)
  })
})
