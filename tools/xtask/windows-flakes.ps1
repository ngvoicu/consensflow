# The check of the Windows runner's own tests for the flakes of T-545, in one run: what a program
# ended from outside leaves as its exit code (windows-exits.ps1), whether this machine gives a
# process id again within a few starts (windows-ids.ps1), and `-Runs` runs of `windows::script`
# with the processor busy and a little churn (windows-load.ps1: at most `-Burners + -Churners`
# processes of its own, each ended by its own clock). It ends with the status of the loaded runs:
# 0 if every one of them passed.
#
#   cargo xtask windows --host zeewin -- powershell -NoProfile -ExecutionPolicy Bypass `
#     -File tools\xtask\windows-flakes.ps1
param(
  [int]$Runs = 20,
  [int]$Burners = 12,
  [int]$Churners = 2,
  [int]$MaxMinutes = 150
)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$powershell = (Get-Command powershell.exe).Source
foreach ($script in 'windows-exits.ps1', 'windows-ids.ps1') {
  Write-Output "== $script"
  & $powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here $script)
}
Write-Output "== windows-load.ps1"
& $powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here 'windows-load.ps1') -Runs $Runs -Burners $Burners -Churners $Churners -MaxMinutes $MaxMinutes
exit $LASTEXITCODE
