#!/usr/bin/env node
/**
 * The portable Windows app: one exe to run from anywhere, without installing.
 * ConsensFlow.exe carries its own `cf`, the daemon and every window's command,
 * and the terminals' console host, after its own bytes, and its first start
 * unpacks them into %LOCALAPPDATA%\dev.ngvoicu.consensflow\portable-runtime
 * (not `runtime`, which the apps before the flip release empty of every
 * runtime whose node.exe is not running).
 * The file's layout, and how the app reads it, are written down once, in
 * app/src-tauri/src/portable.rs. Its data lives where the installed app's
 * does (%USERPROFILE%\.consensflow), and the app installs no update in place
 * on Windows, so a portable copy is never swapped for an installed one.
 *
 * Run after `npm --prefix app run build` on Windows:
 *   node app/scripts/portable.mjs [--release <dir>] [--out <dir>] [--version <x>]
 */
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const RELEASE = join(APP, 'src-tauri', 'target', 'release')
/** The footer's tag: the last eight bytes of an exe that carries its runtime. */
const TAG = 'CFPAYLD1'

const { values } = parseArgs({
  options: {
    release: { type: 'string', default: RELEASE },
    out: { type: 'string', default: join(RELEASE, 'bundle', 'portable') },
    version: { type: 'string' },
  },
})
const version =
  values.version ??
  JSON.parse(readFileSync(join(APP, 'src-tauri', 'tauri.conf.json'), 'utf8')).version

// cli\bin\cf.exe is the daemon and a pane's `cf`: without it the app has no
// daemon to start and a window has no `cf` in PowerShell. conpty.dll and
// OpenConsole.exe are the terminals' console host (scripts/conpty.mjs), which
// the app finds in the runtime folder, with Microsoft's license for them.
const RUNTIME = ['cli', 'conpty.dll', 'OpenConsole.exe', 'OpenConsole-LICENSE.txt']
// What has to be there to pack them: `cli` is a folder of the one file.
const REQUIRED = RUNTIME.map((name) => (name === 'cli' ? join('cli', 'bin', 'cf.exe') : name))
for (const name of ['ConsensFlow.exe', ...REQUIRED]) {
  if (!existsSync(join(values.release, name))) {
    console.error(
      `portable: ${name} is missing from ${values.release}; build first with npm --prefix app run build`,
    )
    process.exit(1)
  }
}

// Windows' own tar.exe, which gzips as the Mac's does; a bare `tar` there may
// be Git's GNU tar, which reads `C:` as a remote host. COPYFILE_DISABLE keeps
// the Mac's from adding a `._` twin of every file.
const tar =
  process.platform === 'win32'
    ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
    : 'tar'
const staging = mkdtempSync(join(tmpdir(), 'cf-portable-'))
try {
  const runtime = join(staging, 'runtime.tar.gz')
  execFileSync(tar, ['-c', '-z', '-f', runtime, '-C', values.release, ...RUNTIME], {
    env: { ...process.env, COPYFILE_DISABLE: '1' },
  })
  const payload = readFileSync(runtime)
  const footer = Buffer.alloc(16)
  footer.writeBigUInt64LE(BigInt(payload.length), 0)
  footer.write(TAG, 8, 'ascii')
  mkdirSync(values.out, { recursive: true })
  const exe = join(values.out, `ConsensFlow_${version}_x64-portable.exe`)
  const app = readFileSync(join(values.release, 'ConsensFlow.exe'))
  writeFileSync(exe, Buffer.concat([app, payload, footer]))
  console.log(`portable: ${exe}`)
} finally {
  rmSync(staging, { recursive: true, force: true })
}
