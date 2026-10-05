/**
 * Runs a command on a Windows machine, over SSH, on this working tree as it
 * is now: every tracked file, and new ones not ignored, go over as one
 * tarball and are unpacked into a build folder there (a file gone from the
 * tree since the last run goes from there too; node_modules and the Rust
 * build stay). The command then runs in that folder with Node and Cargo on
 * PATH, its output streamed back; its exit code is this one's. One run at a
 * time: a second is refused while the first goes on there, and a run stopped
 * here (Ctrl+C) is ended there too, which Windows' SSH server does not do.
 *
 *   npm run windows -- --host <ssh host> [--build] -- npm run live:paste
 *   npm run windows -- --host <ssh host> -- npm run eval -- --scenario round-trip --chief devin --staff devin
 *
 * --build installs the packages and builds the page, a pane's cf.exe and the
 * pane host first: on a fresh machine, and after any change to the Rust. --in runs the
 * command in a folder of the copy (cargo reads app/src-tauri/.cargo there).
 *
 *   npm run windows -- --host <ssh host> --in app/src-tauri -- cargo test The machine needs an
 * OpenSSH server, Node, Rust, and Windows' own tar.exe; the folder is
 * %USERPROFILE%\consensflow-build unless --dir names another under it. Run
 * one at a time: a live run's windows are the machine's.
 */
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    host: { type: 'string' },
    build: { type: 'boolean', default: false },
    dir: { type: 'string', default: 'consensflow-build' },
    in: { type: 'string', default: '.' },
  },
})
if (!values.host || positionals.length === 0) {
  throw new Error('usage: npm run windows -- --host <ssh host> [--build] -- <command>')
}
if (!/^[\w.-]+$/.test(values.dir)) throw new Error(`--dir is a folder name: ${values.dir}`)
if (!/^[\w./-]+$/.test(values.in) || values.in.split('/').includes('..')) {
  throw new Error(`--in is a folder of the copy: ${values.in}`)
}

/** The list of what went over, kept beside it there, so the next run knows what left the tree. */
const SYNCED = '.synced-files'
/** The running script's process id, there, while a run goes on. */
const RUNNING = '.windows-run.pid'
/** A PowerShell script, as ssh hands it to the machine. */
const remote = (script) => [
  values.host,
  `powershell -NoProfile -NonInteractive -EncodedCommand ${Buffer.from(script, 'utf16le').toString('base64')}`,
]
const scratch = mkdtempSync(join(tmpdir(), 'cf-windows-'))
try {
  const files = execFileSync('git', ['ls-files', '-co', '--exclude-standard', '-z'], {
    cwd: REPO,
    encoding: 'utf8',
  })
    .split('\0')
    .filter(Boolean)
  writeFileSync(join(REPO, SYNCED), `${files.join('\n')}\n`)
  const list = join(scratch, 'files')
  writeFileSync(list, [...files, SYNCED].join('\0'))
  const tarball = join(scratch, 'tree.tgz')
  try {
    // COPYFILE_DISABLE: macOS's tar leaves out its ._ resource files.
    execFileSync('tar', ['czf', tarball, '--null', '-T', list], {
      cwd: REPO,
      env: { ...process.env, COPYFILE_DISABLE: '1' },
    })
  } finally {
    rmSync(join(REPO, SYNCED), { force: true })
  }
  execFileSync('scp', ['-q', tarball, `${values.host}:${values.dir}.tgz`], { stdio: 'inherit' })

  // The command as cmd.exe reads a line: an argument with a space or a quote
  // in it quoted, the way programs split their command line. It runs from a
  // file there, so nothing on its way (ssh, PowerShell) reads it again.
  const command = positionals
    .map((arg) => (/^[\w./:=@+,-]+$/.test(arg) ? arg : `"${arg.replace(/"/g, '\\"')}"`))
    .join(' ')
    .replace(/%/g, '%%')
  const steps = values.build
    ? [
        'npm ci --no-audit --no-fund',
        'npm ci --prefix app --no-audit --no-fund',
        'npm --prefix app run bundle:ui',
        'npm --prefix app run prepare-sidecar',
        'npm run build:bridge',
        // The console host the app ships, beside the pane host, as the app has it.
        'node app/scripts/conpty.mjs --into app/src-tauri/target/release',
      ]
    : []
  const script = `
$ProgressPreference = 'SilentlyContinue'
$ErrorActionPreference = 'Stop'
# What the command prints is UTF-8, and goes back as it came.
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch {}
$OutputEncoding = [Text.UTF8Encoding]::new($false)
$dir = Join-Path $env:USERPROFILE '${values.dir}'
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
  const child = spawn('ssh', remote(script), { stdio: ['ignore', 'inherit', 'inherit'] })
  // Stopped here: the run's whole tree is ended there, its record with it.
  const stop = `
$running = Join-Path (Join-Path $env:USERPROFILE '${values.dir}') '${RUNNING}'
if (Test-Path $running) { taskkill /PID (Get-Content $running) /T /F | Out-Null; Remove-Item -LiteralPath $running -Force }
`
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    process.once(signal, () => {
      child.kill()
      spawnSync('ssh', remote(stop), { stdio: 'inherit' })
      rmSync(scratch, { recursive: true, force: true })
      process.exit(130)
    })
  }
  const code = await new Promise((resolve) => child.on('exit', (exit) => resolve(exit ?? 1)))
  process.exitCode = code
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
