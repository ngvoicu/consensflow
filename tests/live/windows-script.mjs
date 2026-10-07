/**
 * What `npm run windows` (tests/live/windows.mjs) has the Windows machine run,
 * as text: nothing here reaches a machine, so a test can read it
 * (tests/windows-script.test.mjs). The one script is PowerShell, handed to
 * ssh encoded; the command to run goes from a file there, so that nothing on
 * its way reads it again.
 */

/** The list of what went over, kept beside it there, so the next run knows what left the tree. */
export const SYNCED = '.synced-files'
/** The running script's process id, there, while a run goes on. */
export const RUNNING = '.windows-run.pid'

/** What `--build` runs first: the packages, the page, a pane's cf.exe and the pane host. */
export const BUILD_STEPS = [
  'npm ci --no-audit --no-fund',
  'npm ci --prefix app --no-audit --no-fund',
  'npm --prefix app run bundle:ui',
  'npm --prefix app run prepare-sidecar',
  'npm run build:bridge',
  // The console host the app ships, beside the pane host, as the app has it.
  'node app/scripts/conpty.mjs --into app/src-tauri/target/release',
]

/**
 * The command as cmd.exe reads a line: an argument with a space or a quote in
 * it quoted, the way programs split their command line.
 */
export function commandLine(words) {
  return words
    .map((arg) => (/^[\w./:=@+,-]+$/.test(arg) ? arg : `"${arg.replace(/"/g, '\\"')}"`))
    .join(' ')
    .replace(/%/g, '%%')
}

/**
 * The variables a run names, `--env NAME=VALUE` each, as `[name, value]`: no
 * others go. A value is printable ASCII, since it is written into the script
 * between single quotes (where only a quote needs saying again) and read back
 * by programs that take it as it is; an empty one takes the variable away
 * there, as PowerShell does.
 */
export function parseEnv(entries) {
  const named = new Map()
  for (const entry of entries) {
    const at = entry.indexOf('=')
    const name = at === -1 ? entry : entry.slice(0, at)
    const value = at === -1 ? '' : entry.slice(at + 1)
    if (at === -1 || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
      throw new Error(`--env is NAME=VALUE, a variable's name and what to give it: ${entry}`)
    }
    if (!/^[\x20-\x7e]*$/.test(value)) {
      throw new Error(`--env ${name} is not printable ASCII: it could not reach the run as it is`)
    }
    if (named.has(name)) throw new Error(`--env ${name} is given twice`)
    named.set(name, value)
  }
  return [...named]
}

/** `text` as PowerShell takes it between single quotes. */
const quoted = (text) => `'${text.replace(/'/g, "''")}'`

/**
 * What the machine's shell reads of a command line: cmd.exe's 8191 characters,
 * less what ssh puts before the script (`powershell -NoProfile -NonInteractive
 * -EncodedCommand `) and a margin. A script with the build's steps takes about
 * 6,100 of it, and each variable a run names about 300 more.
 */
const ENCODED_LIMIT = 8000

/** The script as `powershell -EncodedCommand` takes it, or the error that says it will not fit. */
export function encodeScript(script) {
  const encoded = Buffer.from(script, 'utf16le').toString('base64')
  if (encoded.length > ENCODED_LIMIT) {
    throw new Error(
      `the script for the machine is ${encoded.length} characters encoded, and its shell reads ${ENCODED_LIMIT} at most: name fewer variables with --env, or run a shorter command`,
    )
  }
  return encoded
}

/**
 * The script: the tarball unpacked into `dir` under the user's profile (what
 * left the tree since the last run removed), then `steps` run there, then the
 * variables `env` (from `parseEnv`) given to the process and said in the
 * output, then `command` (from `commandLine`) run from `inside`, a folder of the copy.
 * The steps do not see the variables: they are the command's.
 */
export function remoteScript({ dir, inside, steps, command, env = [] }) {
  const given = env
    .map(
      ([name, value]) =>
        `$env:${name} = ${quoted(value)}\nWrite-Output "== env ${name}=$env:${name}"\n`,
    )
    .join('')
  return `
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
# What the command prints is UTF-8, and goes back as it came.
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
$dir = Join-Path $env:USERPROFILE '${dir}'
New-Item -ItemType Directory -Force $dir | Out-Null
$running = Join-Path $dir '${RUNNING}'
if (Test-Path $running) {
  $other = Get-Process -Id (Get-Content $running) -ErrorAction SilentlyContinue
  if ($other) { Write-Output "A run is still going on this machine (process $($other.Id)): one at a time."; exit 75 }
}
Set-Content -Path $running -Value $PID
try {
$synced = Join-Path $dir '${SYNCED}'
$before = if (Test-Path $synced) { Get-Content $synced } else { @() }
tar.exe -xzf (Join-Path $env:USERPROFILE '${dir}.tgz') -C $dir
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
${given}Set-Location (Join-Path $dir '${inside}')
$run = Join-Path $dir '.windows-run.cmd'
[IO.File]::WriteAllText($run, "@echo off\r\n" + '${command.replace(/'/g, "''")}' + "\r\n")
cmd /c "$run 2>&1"
exit $LASTEXITCODE
} finally {
  Remove-Item -LiteralPath $running -Force -ErrorAction SilentlyContinue
}
`
}
