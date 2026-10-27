#!/bin/bash
# windows-linger.sh <tree> <machine> <label> <how> [seconds]: a run whose command leaves a program running and ends with
# 101 (windows-linger.ps1; or, with <how> as @FILE, the command whose words FILE holds, one to a line), and the machine read every second from this Mac (a read-only ssh, which takes no lock) from
# the run's start until its PowerShell and its records are gone: when the run's process ended, when the owner and process
# files went, and when the lock could be taken. Then a second run straight after the first, through the same queue, to
# see whether it is refused (T-701 item 4). The log is <out>/<label>.log and <out>/<label>.poll.
#   bash tools/xtask/windows-linger.sh windows-machines-3 zeewin2 before cmd-start 150
set -u
T=${WINDOWS_TREES:-/Users/gabrielvoicu/Projects/ngvoicu/consensflow-trees}
tool=${WINDOWS_FROM_TREE:-/Users/gabrielvoicu/Projects/ngvoicu/consensflow-sme/operations/lead-tools/windows-from-tree.sh}
out=${WINDOWS_LINGER_OUT:-$T/.runs/windows-linger}
tree=$1; machine=$2; label=$3; how=${4:-cmd-start}; seconds=${5:-150}
mkdir -p "$out"
log=$out/$label.log; poll=$out/$label.poll; second=$out/$label.second.log
: > "$poll"
ASK='$d = Join-Path $env:USERPROFILE "consensflow-build"; $of = @(Get-Content (Join-Path $d ".windows-run.id") -ErrorAction SilentlyContinue); $p = if ($of.Count -ge 2) { Get-Process -Id ([int]$of[1]) -ErrorAction SilentlyContinue } ; $held = try { $l = [IO.File]::Open((Join-Path $d ".windows-run.lock"), "Open", "ReadWrite", "ReadWrite"); $l.Dispose(); "free" } catch { "HELD" }; "{0} owner-file={1} pid-file={2} process={3} lock={4}" -f (Get-Date -Format HH:mm:ss.fff), (Test-Path (Join-Path $d ".windows-run.id")), (Test-Path (Join-Path $d ".windows-run.pid")), $(if ($p) { "$($p.Id) exited=$($p.HasExited)" } else { "gone" }), $held'
( started=0
  while :; do
    if [ $started = 0 ] && grep -q '^== linger' "$log" 2>/dev/null; then started=1; fi
    if [ $started = 1 ]; then
      printf '%s mac=%s %s\n' "$(date +%T)" "$(date +%s)" "$(ssh -o BatchMode=yes -o ConnectTimeout=8 "$machine" "$ASK" 2>&1 | tr -d '\r' | head -1)" >> "$poll"
    fi
    sleep 1
  done ) &
poller=$!
echo "$(date +%T) first run starts" >> "$out/$label.times"
if [ "${how#@}" != "$how" ]; then
  words=(); while IFS= read -r w; do words+=("$w"); done < "${how#@}"
  # The words do not say the line the poller waits for, so it starts with the run.
  echo "== linger: command from ${how#@}" > "$log"
  bash "$tool" "$tree" --machine "$machine" "${words[@]}" >> "$log" 2>&1
else
  bash "$tool" "$tree" --machine "$machine" powershell -NoProfile -File tools/xtask/windows-linger.ps1 -How "$how" -Seconds "$seconds" > "$log" 2>&1
fi
echo "$(date +%T) first run's Mac side ended with $?" >> "$out/$label.times"
bash "$tool" "$tree" --machine "$machine" --wait 0 cmd /c "echo second run ran" > "$second" 2>&1
echo "$(date +%T) second run ended with $?: $(grep -m1 -E 'second run ran|busy|still going' "$second")" >> "$out/$label.times"
# Until the machine is clear, at most a quarter of an hour.
for i in $(seq 1 900); do
  last=$(tail -1 "$poll")
  case "$last" in *"process=gone"*"lock=free"*) break ;; esac
  sleep 1
done
echo "$(date +%T) machine clear: $last" >> "$out/$label.times"
kill "$poller" 2>/dev/null
