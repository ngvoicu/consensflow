#!/bin/bash
# windows-memory.sh <tree> <machine> <label> [VAR=value ...]: the workspace's tests on a Windows machine through the
# lead tool's queue (windows-from-tree.sh), timed on the machine (the build, then the tests), while this Mac asks the
# machine every 30 s for its processor load and free memory (a read-only ssh, which takes no lock). It says, in
# <out>/summary.txt, the wall time, the build's and the tests' times, the least free memory seen and how many samples
# read under 1 GB, so that a machine's limits (CARGO_BUILD_JOBS, RUST_TEST_THREADS, which the VAR=value words give
# the command as --env) are chosen from numbers (T-701 item 2; T-585 measured zeewin at 0 GB free and 1,396 s).
#   bash tools/xtask/windows-memory.sh windows-machines-3 zeewin jobs4 CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4
# <out> is $WINDOWS_MEMORY_OUT (default: consensflow-trees/.runs/windows-memory).
set -u
T=${WINDOWS_TREES:-/Users/gabrielvoicu/Projects/ngvoicu/consensflow-trees}
tool=${WINDOWS_FROM_TREE:-/Users/gabrielvoicu/Projects/ngvoicu/consensflow-sme/operations/lead-tools/windows-from-tree.sh}
out=${WINDOWS_MEMORY_OUT:-$T/.runs/windows-memory}
tree=$1; machine=$2; label=$3; shift 3
mkdir -p "$out"
env_words=()
for pair in "$@"; do env_words+=(--env "$pair"); done
CMD='$t = Get-Date; cargo test --offline --workspace --exclude app --exclude cf-e2e --no-run; $c = $LASTEXITCODE; Write-Output ("== build took {0:N0} s" -f ((Get-Date) - $t).TotalSeconds); if ($c -ne 0) { exit $c }; $t = Get-Date; cargo test --offline --workspace --exclude app --exclude cf-e2e --no-fail-fast; $c = $LASTEXITCODE; Write-Output ("== tests took {0:N0} s" -f ((Get-Date) - $t).TotalSeconds); exit $c'
SAMPLE='$m = [math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB, 2); $c = (Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average; "cpu {0}% free-mem {1} GB" -f $c, $m'
log=$out/$label.log; load=$out/$label.load
: > "$load"
( seen=0
  while :; do
    if [ $seen = 0 ] && grep -q '^== tree' "$log" 2>/dev/null; then seen=1; date +%s > "$out/$label.tree-at"; fi
    if [ $seen = 1 ]; then
      printf '%s %s\n' "$(date +%T)" "$(ssh -o BatchMode=yes -o ConnectTimeout=8 -o ServerAliveInterval=5 -o ServerAliveCountMax=1 "$machine" "$SAMPLE" 2>/dev/null | tr -d '\r')" >> "$load"
    fi
    sleep 30
  done ) &
sampler=$!
s=$(date +%s)
bash "$tool" "$tree" --machine "$machine" ${env_words[@]+"${env_words[@]}"} powershell -NoProfile -Command "$CMD" > "$log" 2>&1
rc=$?
kill "$sampler" 2>/dev/null
e=$(date +%s); at=$(cat "$out/$label.tree-at" 2>/dev/null || echo "$s")
least=$(awk '{print $5}' "$load" | sort -n | head -1)
low=$(awk '$5 < 1' "$load" | wc -l | tr -d ' ')
echo "$(date +%T) $label $machine rc=$rc wall=$((e - s)) s, from the tree's unpack $((e - at)) s (queued $((at - s)) s) $(grep -E '^== (build|tests) took' "$log" | tr '\n' ' ') least free memory ${least:-?} GB, $low of $(wc -l < "$load" | tr -d ' ') samples under 1 GB, $(grep -c '^test .* FAILED' "$log") tests failed" >> "$out/summary.txt"
exit $rc
