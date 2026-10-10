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
 * pane host first: on a fresh machine, and after any change to the Rust. --in
 * runs the command in a folder of the copy.
 *
 *   npm run windows -- --host <ssh host> --in app/src-tauri -- cargo test
 *
 * --env NAME=VALUE gives the command that variable, as many as are named and
 * no others: this machine's environment does not go, and the machine's own is
 * the command's as it is. The output says each (`== env NAME=VALUE`) before
 * the command's own. It is how the evals and the live checks that start a
 * daemon (tests/choice.mjs) start the native one there, with the native cf the
 * copy built (bin\cf.exe: --build builds it). The black-box suites of
 * crates/cf-e2e build the cf and the pane host they run, and need none:
 *
 *   npm run windows -- --host <ssh host> -- npm run test:integration
 *   npm run windows -- --host <ssh host> --env CONSENSFLOW_TEST_DAEMON=native -- npm run eval -- --scenario round-trip --chief devin --staff devin
 *
 * `npm run eval:windows` (tests/live/windows-matrix.mjs) takes --env too, and
 * gives it to each of its runs. The machine needs an OpenSSH server, Node,
 * Rust, and Windows' own tar.exe; the folder is %USERPROFILE%\consensflow-build
 * unless --dir names another under it. Run one at a time: a live run's windows
 * are the machine's.
 */
import { execFileSync, spawn, spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import {
  BUILD_STEPS,
  commandLine,
  encodeScript,
  parseEnv,
  RUNNING,
  remoteScript,
  SYNCED,
} from './windows-script.mjs'

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    host: { type: 'string' },
    build: { type: 'boolean', default: false },
    dir: { type: 'string', default: 'consensflow-build' },
    in: { type: 'string', default: '.' },
    env: { type: 'string', multiple: true, default: [] },
  },
})
if (!values.host || positionals.length === 0) {
  throw new Error(
    'usage: npm run windows -- --host <ssh host> [--build] [--env NAME=VALUE] -- <command>',
  )
}
if (!/^[\w.-]+$/.test(values.dir)) throw new Error(`--dir is a folder name: ${values.dir}`)
if (!/^[\w./-]+$/.test(values.in) || values.in.split('/').includes('..')) {
  throw new Error(`--in is a folder of the copy: ${values.in}`)
}
const environment = parseEnv(values.env)

/** A PowerShell script, as ssh hands it to the machine. */
const remote = (script) => [
  values.host,
  `powershell -NoProfile -NonInteractive -EncodedCommand ${encodeScript(script)}`,
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

  // The command as cmd.exe reads a line. It runs from a file there, so nothing
  // on its way (ssh, PowerShell) reads it again.
  const command = commandLine(positionals)
  const script = remoteScript({
    dir: values.dir,
    inside: values.in,
    steps: values.build ? BUILD_STEPS : [],
    command,
    env: environment,
  })
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
