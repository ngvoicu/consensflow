#!/usr/bin/env node
// What Codex 0.167.0's own key reader makes of the bytes a pane writes, on Windows (T-1348), through
// the console host the app ships (ConPTY 1.25 beside its OpenConsole.exe) and the pane host's own
// write path: builds the probe `codex-windows-keys` (crossterm's EventStream inside Codex's
// WindowsKeySequence, copied as it is at rust-v0.167.0), then runs the three `#[ignore]`d tests of
// `cf-panes` that drive it (`pty::keys::conpty::codex_on_windows_*`). Offline: no harness starts, no
// model runs. From the tree's root, on a Windows machine:
//
//     node tools/probes/key-reader/run-windows.cjs
//
// or from a Mac: `windows-from-tree.sh <tree> --machine zeewin2 --wait 240 node tools/probes/key-reader/run-windows.cjs`.
// (.cjs: the tree's package.json says "type": "module".) Needs `cargo xtask conpty --into
// app/src-tauri/target/debug` done first, as the Windows runner does for every run. Each case
// prints a `CASE <json>` line: results-windows.json is those lines, recorded.
const { spawnSync } = require('node:child_process')
const path = require('node:path')

const root = process.cwd()
const target = path.join(root, 'app', 'src-tauri', 'target', 'probes', 'key-reader')
const probe = path.join(target, 'debug', 'codex-windows-keys.exe')

const step = (what, args, env = {}) => {
  const started = Date.now()
  const run = spawnSync('cargo', args, {
    stdio: 'inherit',
    shell: true,
    env: { ...process.env, ...env },
  })
  const seconds = ((Date.now() - started) / 1000).toFixed(1)
  console.log(`== ${what}: exit ${run.status} in ${seconds} s`)
  return run.status
}

let status = step(
  'build the probe',
  ['build', '--manifest-path', path.join('tools', 'probes', 'key-reader', 'Cargo.toml'), '--bin', 'codex-windows-keys'],
  { CARGO_TARGET_DIR: target },
)
if (status === 0) {
  status = step(
    'run the tests that drive it',
    ['test', '-p', 'cf-panes', '--lib', '--', '--ignored', '--nocapture', '--test-threads=1', 'pty::keys::conpty::codex_on_windows'],
    { CF_CODEX_KEY_PROBE: probe },
  )
}
process.exit(status === 0 ? 0 : 1)
