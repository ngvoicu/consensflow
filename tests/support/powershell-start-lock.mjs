import { spawn } from 'node:child_process'
import path from 'node:path'

// A run may name its own file. Otherwise all runs use the same persistent Temp
// file. Each holder locks the opened file by handle and never removes it.
const lockVariable = 'CF_PWSH_START_LOCK'
export function pwshEnvironment(environment) {
  // .NET's profile recorder writes at process exit, after the start turn can
  // have ended. Disable that recorder and avoid creating telemetry cache files.
  const clean = { ...environment }
  for (const name of Object.keys(clean)) {
    if (
      [
        'DOTNET_MULTICOREJITMINNUMCPUS',
        'COMPLUS_MULTICOREJITMINNUMCPUS',
        'POWERSHELL_TELEMETRY_OPTOUT',
      ].includes(name.toUpperCase())
    )
      delete clean[name]
  }
  return {
    ...clean,
    DOTNET_MultiCoreJitMinNumCpus: '65535',
    COMPlus_MultiCoreJitMinNumCpus: '65535',
    POWERSHELL_TELEMETRY_OPTOUT: '1',
  }
}
const unixGuard = `import fcntl, os, sys, time
fd = os.open(os.environ['CF_PWSH_START_LOCK'], os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
with os.fdopen(fd, 'r+b') as lock:
    until = time.monotonic() + 60
    while True:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except BlockingIOError:
            if time.monotonic() >= until: raise
            time.sleep(0.02)
    print('locked', flush=True)
    sys.stdin.buffer.read(1)
`
const windowsGuard = `
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class CfPwshLock {
  [StructLayout(LayoutKind.Sequential)] public struct AttributeTag { public uint Attributes; public uint Tag; }
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern SafeFileHandle CreateFileW(string path, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool GetFileInformationByHandleEx(SafeFileHandle file, int kind, out AttributeTag info, uint size);
}
'@
# OPEN_ALWAYS, FILE_FLAG_OPEN_REPARSE_POINT: inspect and lock the opened object,
# without following a final symlink or junction even if the name changes.
$handle = [CfPwshLock]::CreateFileW($env:CF_PWSH_START_LOCK, [uint32]3221225472, 3, [IntPtr]::Zero, 4, 0x00200000, [IntPtr]::Zero)
if ($handle.IsInvalid) { throw "could not open the PowerShell start lock: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
$info = New-Object 'CfPwshLock+AttributeTag'
if (-not [CfPwshLock]::GetFileInformationByHandleEx($handle, 9, [ref]$info, [Runtime.InteropServices.Marshal]::SizeOf($info))) { $handle.Dispose(); throw 'could not inspect the PowerShell start lock' }
if ($info.Attributes -band 0x400) { $handle.Dispose(); throw 'the PowerShell start lock is a reparse point' }
$file = [IO.FileStream]::new($handle, [IO.FileAccess]::ReadWrite)
try {
  $watch = [Diagnostics.Stopwatch]::StartNew()
  while ($true) {
    try { $file.Lock(0, 1); break }
    catch [IO.IOException] { if ($watch.Elapsed.TotalSeconds -ge 60) { throw }; Start-Sleep -Milliseconds 20 }
  }
  [Console]::Out.WriteLine('locked')
  [Console]::Out.Flush()
  $null = [Console]::In.Read()
} finally { $file.Dispose() }
`

async function takeTurn() {
  const windows = process.platform === 'win32'
  const system = process.env.SystemRoot
  if (windows && !system) throw new Error('SystemRoot is not set')
  const temp = windows
    ? process.env.TEMP || process.env.TMP || path.join(system, 'Temp')
    : process.env.TMPDIR || process.env.TMP || process.env.TEMP || '/tmp'
  const lock = process.env[lockVariable] || path.join(temp, 'consensflow-pwsh-start-v1.lock')
  const program = windows
    ? path.join(system, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')
    : 'python3'
  const args = windows
    ? ['-NoProfile', '-NonInteractive', '-Command', windowsGuard]
    : ['-u', '-c', unixGuard]
  const child = spawn(program, args, {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, [lockVariable]: lock },
  })
  child.stdin.on('error', () => {})
  const closed = new Promise((resolve) =>
    child.once('close', (code, signal) => resolve({ code, signal })),
  )
  let errors = ''
  child.stderr.on('data', (chunk) => {
    errors += chunk
  })
  const ready = new Promise((resolve, reject) => {
    let out = ''
    child.stdout.on('data', (chunk) => {
      out += chunk
      if (out.includes('locked\n') || out.includes('locked\r\n')) resolve()
    })
    child.once('error', reject)
    child.once('close', () =>
      reject(new Error(`PowerShell start lock ended before it was held: ${errors}`)),
    )
  })
  try {
    await ready
    return { child, closed }
  } catch (error) {
    child.stdin.end('x')
    await closed
    throw error
  }
}

// A turn ends once the spawned PowerShell reports that its script has begun
// (or the process fails before then). The guard releases its handle on EOF,
// including if its owner dies before it can send the byte.
export class PowerShellStartLock {
  async start(start) {
    const { child, closed } = await takeTurn()
    let releasing = false
    const lost = closed.then(({ code, signal }) => {
      if (!releasing) throw new Error(`PowerShell start lock was lost: ${code ?? signal}`)
    })
    try {
      const started = start()
      await Promise.race([started.ready, lost])
      return started.value
    } finally {
      releasing = true
      child.stdin.end('x')
      await closed
    }
  }
}

export function watchReady(stream, marker, accept) {
  return new Promise((resolve) => {
    let pending = ''
    let ready = false
    stream.setEncoding('utf8')
    stream.on('data', (chunk) => {
      if (ready) {
        accept(chunk)
        return
      }
      pending += chunk
      const at = pending.indexOf(marker)
      if (at >= 0) {
        accept(pending.slice(0, at))
        const rest = pending.slice(at + marker.length)
        pending = ''
        ready = true
        resolve()
        if (rest) accept(rest)
      } else if (pending.length >= marker.length) {
        const safe = pending.length - marker.length + 1
        accept(pending.slice(0, safe))
        pending = pending.slice(safe)
      }
    })
    stream.on('end', () => {
      if (!ready) accept(pending)
      resolve()
    })
    stream.on('error', resolve)
  })
}
