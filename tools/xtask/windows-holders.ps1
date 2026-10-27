# Says which processes hold a file open, by the Restart Manager (the system's own list of who uses a file), and
# whether the file can be opened for the run alone, as the lock is. It is for a lock a run finds held when no run is
# (T-701, item 4): who has it, how long they have lived, and what their parents are.
#
#   powershell -NoProfile -File tools\xtask\windows-holders.ps1 [-Path <file>]
param([string]$Path = (Join-Path $env:USERPROFILE 'consensflow-build\.windows-run.lock'))
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public static class Holders {
  [StructLayout(LayoutKind.Sequential)] struct FILETIME { public uint Low, High; }
  [StructLayout(LayoutKind.Sequential)] struct UNIQUE { public int Pid; public FILETIME Start; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct INFO {
    public UNIQUE Process; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string App;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string Service; public int Type, Status, Session;
    [MarshalAs(UnmanagedType.Bool)] public bool Restartable; }
  [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)] static extern int RmStartSession(out uint h, int flags, string key);
  [DllImport("rstrtmgr.dll")] static extern int RmEndSession(uint h);
  [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)] static extern int RmRegisterResources(uint h, uint nf, string[] files, uint na, IntPtr apps, uint ns, string[] services);
  [DllImport("rstrtmgr.dll")] static extern int RmGetList(uint h, out uint needed, ref uint have, [In, Out] INFO[] info, out uint reasons);
  public static List<string> Of(string path) {
    var found = new List<string>(); uint h; string key = Guid.NewGuid().ToString();
    if (RmStartSession(out h, 0, key) != 0) { found.Add("RmStartSession failed"); return found; }
    try {
      if (RmRegisterResources(h, 1, new[] { path }, 0, IntPtr.Zero, 0, null) != 0) { found.Add("RmRegisterResources failed"); return found; }
      uint needed = 0, have = 0, reasons; int r = RmGetList(h, out needed, ref have, null, out reasons);
      if (r == 0 && needed == 0) return found;
      var info = new INFO[needed + 4]; have = (uint)info.Length; r = RmGetList(h, out needed, ref have, info, out reasons);
      if (r != 0) { found.Add("RmGetList failed: " + r); return found; }
      for (int i = 0; i < have; i++) found.Add(info[i].Process.Pid + " " + info[i].App);
    } finally { RmEndSession(h); }
    return found;
  }
}
'@
Write-Output "== $Path"
try { $l = [IO.File]::Open($Path, 'OpenOrCreate', 'ReadWrite', 'None'); Write-Output 'open for the run alone: yes'; $l.Dispose() } catch { Write-Output "open for the run alone: no ($($_.Exception.Message))" }
foreach ($line in [Holders]::Of($Path)) {
  Write-Output "holder: $line"
  $id = [int]($line.Split(' ')[0])
  $p = Get-CimInstance Win32_Process -Filter "ProcessId = $id" -ErrorAction SilentlyContinue
  if ($p) { Write-Output ("  started $($p.CreationDate), parent $($p.ParentProcessId), $($p.CommandLine)") }
}
