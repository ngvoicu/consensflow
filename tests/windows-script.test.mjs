import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  BUILD_STEPS,
  commandLine,
  encodeScript,
  parseEnv,
  remoteScript,
} from './live/windows-script.mjs'

/**
 * What `npm run windows` has the machine run is text, and nothing here reaches
 * a machine: that the variables a run names get to its command, and no others,
 * is read in the script. The Windows run of it is the lead's.
 */

const BUILD = ['npm ci --no-audit --no-fund', 'npm run build:bridge']
const script = (more = {}) =>
  remoteScript({
    dir: 'consensflow-build',
    inside: '.',
    steps: [],
    command: commandLine(['npm', 'run', 'test:integration']),
    ...more,
  })

/** The lines of a script that give the process a variable of its own, by name (PATH is the script's). */
const given = (text) =>
  text
    .split('\n')
    .filter((line) => /^\$env:(?!Path\b)\w+ = /.test(line))
    .map((line) => line.replace(/ = .*$/, '').slice('$env:'.length))

describe('the variables a Windows run names', () => {
  it('are NAME=VALUE, each a variable of its own with the value it is given', () => {
    assert.deepEqual(parseEnv([]), [])
    assert.deepEqual(parseEnv(['CONSENSFLOW_TEST_DAEMON=native']), [
      ['CONSENSFLOW_TEST_DAEMON', 'native'],
    ])
    // The first = ends the name; the value is the rest, a JSON array's quotes and commas too.
    assert.deepEqual(
      parseEnv(['A=b=c', 'B_2=["bin/cf.exe","ui","--json"]', '_x=', "C=it's 100%"]),
      [
        ['A', 'b=c'],
        ['B_2', '["bin/cf.exe","ui","--json"]'],
        ['_x', ''],
        ['C', "it's 100%"],
      ],
    )
  })

  it('are refused when they are not that, before anything is sent', () => {
    for (const entry of ['NOVALUE', '=x', '1A=x', 'A-B=x', 'A B=x', 'É=x', '']) {
      assert.throws(() => parseEnv([entry]), /--env is NAME=VALUE/, JSON.stringify(entry))
    }
    for (const value of ['a\nb', 'a\rb', 'a\tb', 'café', 'a\u0000b', 'smart ‘quote’']) {
      assert.throws(
        () => parseEnv([`A=${value}`]),
        /--env A is not printable ASCII/,
        JSON.stringify(value),
      )
    }
    assert.throws(() => parseEnv(['A=1', 'B=2', 'A=3']), /--env A is given twice/)
  })
})

describe('the script a Windows run is', () => {
  it('carries the variables a run names to its command, after the build steps and before it', () => {
    const text = script({
      steps: BUILD,
      env: parseEnv(['CONSENSFLOW_TEST_DAEMON=native', 'CONSENSFLOW_TEST_CLI=native']),
    })
    assert.deepEqual(given(text), ['CONSENSFLOW_TEST_DAEMON', 'CONSENSFLOW_TEST_CLI'])
    assert.ok(text.includes("$env:CONSENSFLOW_TEST_DAEMON = 'native'\n"))
    assert.ok(text.includes("$env:CONSENSFLOW_TEST_CLI = 'native'\n"))
    const at = (what) => text.indexOf(what)
    assert.ok(
      at('foreach ($step in') < at('$env:CONSENSFLOW_TEST_DAEMON ='),
      'not before the steps',
    )
    assert.ok(at('$env:CONSENSFLOW_TEST_CLI =') < at("Set-Location (Join-Path $dir '.')"))
    assert.ok(
      at("Set-Location (Join-Path $dir '.')") < at('$run = Join-Path'),
      'before the command',
    )
    assert.ok(at('$env:CONSENSFLOW_TEST_DAEMON =') < at('cmd /c "$run 2>&1"'))
    // The run's own output says what it was given, as the first thing after the build.
    assert.ok(
      text.includes('Write-Output "== env CONSENSFLOW_TEST_DAEMON=$env:CONSENSFLOW_TEST_DAEMON"'),
    )
  })

  it('gives its command no variable that was not named', () => {
    for (const none of [undefined, []]) {
      const text = script({ steps: BUILD, ...(none === undefined ? {} : { env: none }) })
      assert.deepEqual(given(text), [])
      assert.ok(!text.includes('== env '))
    }
    // Only what is named: this machine's environment is not in the script.
    assert.deepEqual(given(script({ env: parseEnv(['A=1']) })), ['A'])
  })

  it('says a quote in a value again, and takes everything else in it as it is', () => {
    const text = script({
      env: parseEnv(["A=it's", 'B=["bin/cf.exe","ui"]', 'C=$x `y` %z% "q" & |']),
    })
    assert.ok(text.includes("$env:A = 'it''s'\n"))
    assert.ok(text.includes('$env:B = \'["bin/cf.exe","ui"]\'\n'))
    assert.ok(text.includes('$env:C = \'$x `y` %z% "q" & |\'\n'))
  })

  it('keeps the order the variables were named in', () => {
    assert.deepEqual(given(script({ env: parseEnv(['Z=1', 'A=2', 'M=3']) })), ['Z', 'A', 'M'])
  })

  it('runs the command as cmd.exe reads a line, from a file, with its percent signs doubled', () => {
    assert.equal(commandLine(['npm', 'run', 'live:paste']), 'npm run live:paste')
    assert.equal(commandLine(['echo', 'a b', 'c"d', '%PATH%']), 'echo "a b" "c\\"d" "%%PATH%%"')
    const text = script({ command: commandLine(['echo', "it's"]) })
    assert.ok(text.includes(`'echo "it''s"'`), text)
  })
})

/**
 * The script as tests/live/windows.mjs made it before a run could name
 * variables (6196368f), copied as it was. It is PowerShell that no run here can
 * try: the one protection it has from an edit nobody meant is that, with no
 * variable named, the machine is sent this text, byte for byte. A change meant
 * is made here too.
 */
function sentBefore(values, steps, command) {
  return `
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
# What the command prints is UTF-8, and goes back as it came.
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
$dir = Join-Path $env:USERPROFILE '${values.dir}'
New-Item -ItemType Directory -Force $dir | Out-Null
$running = Join-Path $dir '.windows-run.pid'
if (Test-Path $running) {
  $other = Get-Process -Id (Get-Content $running) -ErrorAction SilentlyContinue
  if ($other) { Write-Output "A run is still going on this machine (process $($other.Id)): one at a time."; exit 75 }
}
Set-Content -Path $running -Value $PID
try {
$synced = Join-Path $dir '.synced-files'
$before = if (Test-Path $synced) { Get-Content $synced } else { @() }
tar.exe -xzf (Join-Path $env:USERPROFILE '${values.dir}.tgz') -C $dir
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$now = [Collections.Generic.HashSet[string]]::new([string[]](Get-Content $synced))
foreach ($file in $before) {
  if (-not $now.Contains($file)) { Remove-Item -LiteralPath (Join-Path $dir $file) -Force -ErrorAction SilentlyContinue }
}
$env:Path = "C:\\Program Files\\nodejs;$env:USERPROFILE\\.cargo\\bin;" + $env:Path
# Codex's own PATH entry is a junction an SSH session may not cross ("an
# untrusted mount point"): its release folder, named outright.
$codex = (Get-Item "$env:USERPROFILE\\.codex\\packages\\standalone\\current" -ErrorAction SilentlyContinue).Target
if ($codex) { $env:Path = "$(@($codex)[0])\\bin;" + $env:Path }
Set-Location $dir
$ErrorActionPreference = 'Continue'
foreach ($step in @(${steps.map((step) => `'${step}'`).join(', ')})) {
  Write-Output "== $step"
  cmd /c "$step 2>&1"
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
Set-Location (Join-Path $dir '${values.in}')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off\r\n" + '${command.replace(/'/g, "''")}' + "\r\n")
cmd /c "$run 2>&1"
exit $LASTEXITCODE
} finally {
  Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue
}
`
}

/** The command as that windows.mjs read it from its arguments. */
const commandBefore = (words) =>
  words
    .map((arg) => (/^[\w./:=@+,-]+$/.test(arg) ? arg : `"${arg.replace(/"/g, '\\"')}"`))
    .join(' ')
    .replace(/%/g, '%%')

describe('a Windows run that names no variable', () => {
  it('is sent the script, and the command line, it was sent before there was --env', () => {
    for (const [words, dir, inside, steps] of [
      [['npm', 'run', 'live:paste'], 'consensflow-build', '.', []],
      [
        ['npm', 'run', 'eval', '--', '--scenario', 'round-trip', '--chief', 'devin'],
        'consensflow-build',
        '.',
        BUILD_STEPS,
      ],
      [['cargo', 'test', 'it'], 'build two', 'app/src-tauri', []],
      [['node', '-e', `console.log("it's 100%")`], 'consensflow-build', '.', []],
      [
        ['echo', "a'b", 'c d', 'e"f', '%PATH%'],
        'consensflow-build',
        'tests',
        BUILD_STEPS.slice(0, 2),
      ],
    ]) {
      const command = commandLine(words)
      assert.equal(command, commandBefore(words), words.join(' '))
      assert.equal(
        remoteScript({ dir, inside, steps, command }),
        sentBefore({ dir, in: inside }, steps, command),
        words.join(' '),
      )
    }
  })
})

describe('the script as the machine is sent it', () => {
  /** The longest run there is: the build, a command as long as an eval's. */
  const eval_ = commandLine([
    ...['npm', 'run', 'eval', '--', '--scenario', 'question-trip', '--chief', 'claude'],
    ...['--staff', 'devin', '--timeout-min', '30', '--claude-model', 'claude-sonnet-5'],
  ])
  const long = (env) => script({ steps: BUILD_STEPS, command: eval_, env: parseEnv(env) })

  it('goes encoded as PowerShell reads it, and comes back as it was', () => {
    const text = long(['CONSENSFLOW_TEST_DAEMON=native'])
    assert.equal(Buffer.from(encodeScript(text), 'base64').toString('utf16le'), text)
  })

  it('takes the build and the variables a run names in practice, with room to spare', () => {
    for (const env of [
      [],
      ['CONSENSFLOW_TEST_DAEMON=native'],
      ['CONSENSFLOW_TEST_DAEMON=native', 'CONSENSFLOW_TEST_CLI=native'],
    ]) {
      assert.doesNotThrow(() => encodeScript(long(env)), env.join(' '))
    }
  })

  it('is refused, saying why, when the machine’s shell could not read the line it makes', () => {
    const many = Array.from(
      { length: 12 },
      (_, at) => `CONSENSFLOW_TEST_VARIABLE_${at}=${'x'.repeat(40)}`,
    )
    assert.throws(
      () => encodeScript(long(many)),
      /the script for the machine is \d+ characters encoded, and its shell reads 8000 at most: name fewer variables with --env, or run a shorter command/,
    )
  })
})

describe('the build a Windows run starts with', () => {
  it('stages the bundle and fetches the console host through cargo xtask, not through the scripts', () => {
    assert.ok(BUILD_STEPS.includes('cargo xtask stage'))
    assert.ok(BUILD_STEPS.includes('cargo xtask conpty --into app/src-tauri/target/release'))
    for (const step of BUILD_STEPS) {
      assert.doesNotMatch(step, /app\/scripts\/(conpty|prepare-sidecar|build-cf|portable)/, step)
      assert.doesNotMatch(step, /prepare-sidecar/, step)
    }
  })

  it('keeps the page before the bundle it is staged into, and the pane host before the console host beside it', () => {
    const at = (step) => BUILD_STEPS.indexOf(step)
    assert.ok(at('npm --prefix app run bundle:ui') < at('cargo xtask stage'))
    assert.ok(
      at('npm run build:bridge') < at('cargo xtask conpty --into app/src-tauri/target/release'),
    )
  })

  it('is a line the machine’s shell reads whole, as the lines of the other steps are', () => {
    for (const step of BUILD_STEPS) assert.equal(commandLine(step.split(' ')), step)
  })
})
