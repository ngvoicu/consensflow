#!/bin/bash
# windows-silent.sh <tree> [seconds]: what a run's connection that goes silent leaves on zeewin (T-804). A run of
# `ping -n <seconds> 127.0.0.1` (300 by default) goes through the lead tool as any run does, over an ssh whose
# connection is made through a relay (`nc`) that is stopped (SIGSTOP) once the command has said its first word: the
# peer then says nothing and answers nothing, and ssh gives it up after the bounds of the runner (`reach.rs`: 135 s
# after its last word), says `Timeout, server … not responding.` and ends with 255. Every connection made after the
# freeze hangs (the relay is a `sleep`), so the machine cannot be asked to end the run: the case of a laptop that is off,
# or a tunnel that does not come back. The machine is read every 5 s from this Mac (a read-only ssh, which takes no
# lock) with the question of windows-linger.sh: when the run's process and lock go. Then a second run, straight after
# the first, told not to wait (--wait 0), to see what the next run is told. The log is <out>/run.log, the
# machine's states <out>/poll.log, the second run's <out>/second.log, the times <out>/times.log.
#   bash tools/xtask/windows-silent.sh windows-machines-3 300
set -u
T=${WINDOWS_TREES:-/Users/gabrielvoicu/Projects/ngvoicu/consensflow-trees}
tool=${WINDOWS_FROM_TREE:-/Users/gabrielvoicu/Projects/ngvoicu/consensflow-sme/operations/lead-tools/windows-from-tree.sh}
out=${WINDOWS_SILENT_OUT:-$T/.runs/windows-silent}
tree=$1; seconds=${2:-300}
machine=zeewin
rm -rf "$out"; mkdir -p "$out/bin" "$out/state"
log=$out/run.log; poll=$out/poll.log; second=$out/second.log; times=$out/times.log
: > "$poll"; : > "$times"
cat > "$out/bin/relay.sh" <<'RELAY'
#!/bin/sh
# relay.sh HOST PORT STATE: the connection ssh asks for, through nc, whose process is written down to be stopped.
nc "$1" "$2" <&0 &
pid=$!
echo "$pid" > "$3/nc.$pid"
trap 'kill -9 $pid 2>/dev/null; exit 0' HUP TERM INT
wait $pid
RELAY
cat > "$out/bin/ssh" <<WRAPPER
#!/bin/sh
# The runner's ssh, first on the PATH: the same words, through the relay; through a hanging one after the freeze.
if [ -e "$out/state/frozen" ]; then proxy="sleep 600"; else proxy="$out/bin/relay.sh %h %p $out/state"; fi
exec /usr/bin/ssh -o "ProxyCommand=\$proxy" "\$@" # ssh-bounds: given by the caller
WRAPPER
chmod +x "$out/bin/relay.sh" "$out/bin/ssh"
ASK='$d = Join-Path $env:USERPROFILE "consensflow-build"; $of = @(Get-Content (Join-Path $d ".windows-run.id") -ErrorAction SilentlyContinue); $p = if ($of.Count -ge 2) { Get-Process -Id ([int]$of[1]) -ErrorAction SilentlyContinue } ; $held = try { $l = [IO.File]::Open((Join-Path $d ".windows-run.lock"), "Open", "ReadWrite", "ReadWrite"); $l.Dispose(); "free" } catch { "HELD" }; "{0} owner-file={1} process={2} lock={3}" -f (Get-Date -Format HH:mm:ss.fff), (Test-Path (Join-Path $d ".windows-run.id")), $(if ($p) { "$($p.Id) exited=$($p.HasExited)" } else { "gone" }), $held'
echo "$(date +%T) run starts" >> "$times"
PATH="$out/bin:$PATH" bash "$tool" "$tree" --machine "$machine" cmd /c ping -n "$seconds" 127.0.0.1 > "$log" 2>&1 &
run=$!
for _ in $(seq 1 36000); do
  grep -q '^windows-from-tree: on '"$machine" "$log" 2>/dev/null && break
  kill -0 "$run" 2>/dev/null || break
  sleep 1
done
echo "$(date +%T) the run has the machine" >> "$times"
( while :; do
    printf '%s mac=%s %s\n' "$(date +%T)" "$(date +%s)" "$(/usr/bin/ssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=5 -o ServerAliveCountMax=1 "$machine" "$ASK" 2>&1 | tr -d '\r' | head -1)" >> "$poll"
    sleep 5
  done ) &
poller=$!
for _ in $(seq 1 3600); do
  grep -q '^Pinging' "$log" 2>/dev/null && break
  kill -0 "$run" 2>/dev/null || break
  sleep 1
done
if grep -q '^Pinging' "$log" 2>/dev/null; then
  : > "$out/state/frozen"
  for pid in "$out"/state/nc.*; do kill -STOP "${pid##*/nc.}" 2>/dev/null; done
  echo "$(date +%T) mac=$(date +%s) the relay is stopped: the peer says nothing" >> "$times"
fi
wait "$run"; ended=$?
echo "$(date +%T) mac=$(date +%s) the run's Mac side ended with $ended" >> "$times"
for pid in "$out"/state/nc.*; do kill -9 "${pid##*/nc.}" 2>/dev/null; done
# From the tree itself, not through the tool, which takes no --wait: told not to wait, the run is refused at once
# while a run goes on there, and sends nothing. It is made only while the machine is held (the last look).
case "$(tail -1 "$poll")" in
  *"lock=HELD"*)
    (cd "$T/$tree" && cargo xtask windows --machine "$machine" --wait 0 -- cmd /c "echo second run ran") > "$second" 2>&1
    status=$?
    echo "$(date +%T) the second run ended with $status: $(grep -m1 -E 'second run ran|busy|still going' "$second")" >> "$times" ;;
  *) echo "$(date +%T) the machine was not held by the time the first run's Mac side had ended: no second run" >> "$times" ;;
esac
# Until the machine is clear, at most a quarter of an hour.
for _ in $(seq 1 180); do
  last=$(tail -1 "$poll")
  case "$last" in *"process=gone"*"lock=free"*) break ;; esac
  sleep 5
done
echo "$(date +%T) machine clear: $last" >> "$times"
kill "$poller" 2>/dev/null
