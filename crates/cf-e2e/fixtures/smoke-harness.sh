#!/bin/sh
# The smoke's stand-in harness. It exists to be recognisable on screen, to
# prove that what the human types reaches a real child — every line it reads
# comes back as hex, which no echo, no replay and no cached frame could
# produce — and, on request, to out-run the output window.
#
# It also keeps the two records a Claude window keeps, because the daemon reads
# them before it delivers and after: sessions/<pid>.json says the window is
# idle, so a delivery may go in, and the transcript holds every line the
# window took as a user turn answered by an assistant turn, which confirms it.
#
# Every window of the smoke runs this script, the chief's first and then the
# worker's, and each appends its pid: the worker's window closes once its
# work is done, the chief's lives as long as the app.
#
# The flood is asked for rather than printed at start-up: 1.5 MiB of wrapped
# lines pushes far more rows than xterm keeps, so a banner printed before it
# is gone by the time anything can look for it. The page says when it has
# seen the banner; only then does the flood run. The flood's size, in lines and
# in columns, is filled in by the smoke where the loops below name it.
LC_ALL=C
export LC_ALL
echo $$ >> "$CFSMOKE_PIDFILE"
session=''
seed=''
while [ $# -gt 0 ]; do
  case "$1" in
    --session-id|--resume) session="$2"; shift ;;
    # A worker's window opens with its brief as the last argument.
    '[ConsensFlow'*) seed="$1" ;;
  esac
  shift
done
config="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
transcript="$config/projects/smoke/$session.jsonl"
status="$config/sessions/$$.json"
mkdir -p "$config/projects/smoke" "$config/sessions"
trap 'rm -f "$status"' EXIT
n=0
stamp() { date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo 1970-01-01T00:00:00Z; }
state() {
  printf '{"pid":%s,"sessionId":"%s","kind":"interactive","status":"%s"}' "$$" "$session" "$1" > "$status"
}
record() {
  n=$((n + 1))
  printf '{"sessionId":"%s","version":"2.1.277","timestamp":"%s","uuid":"%s-%s-%s",%s}\n' \
    "$session" "$(stamp)" "$session" "$$" "$n" "$1" >> "$transcript"
}
# One line read is one turn, the way the integration suite's fake agent does it.
turn() {
  state busy
  record "\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"$1\"}"
  record "\"type\":\"assistant\",\"message\":{\"id\":\"$session-message-$$-$n\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"noted\"}],\"stop_reason\":\"end_turn\"}"
  record "\"type\":\"system\",\"subtype\":\"stop_hook_summary\",\"preventedContinuation\":false,\"hookCount\":1"
  state idle
}
state idle
printf 'CFSMOKE-READY %s\n' "$CFSMOKE_TAG"
# Says whether the pane inherited a usable PATH. A chief whose PATH holds only
# ConsensFlow's own directories cannot run git, ripgrep or any of what a real
# harness shells out to, and every test that stubs the environment would still
# pass. One system command settles it.
if command -v uname >/dev/null 2>&1; then
  printf 'CFSMOKE-TOOLS ok\n'
else
  printf 'CFSMOKE-TOOLS missing\n'
fi
# A worker's first turn is its brief, answered at once: its header line is
# the record the daemon looks for, and "noted" is its result.
if [ -n "$seed" ]; then
  nl='
'
  turn "${seed%%"$nl"*}"
fi
pad=''
n=0
while [ $n -lt @FLOOD_WIDTH@ ]; do
  pad="${pad}x"
  n=$((n + 1))
done
n=0
esc=$(printf '\033')
while IFS= read -r line; do
  if [ "$line" = "BIGPASTE" ]; then
    # The paste is read off the raw terminal by a program that is not a shell:
    # it says it is ready, and then the size and the hash of what it was given.
    saved=$(stty -g)
    stty raw -echo
    "$CFSMOKE_PASTE_READER"
    stty "$saved"
  elif [ "$line" = "HANDOFF" ]; then
    # The chief puts a task on the board, the way a real chief does; a worker
    # window on this same stand-in does it, and the daemon delivers its result
    # into this window.
    cf task add --tier standard "SMOKE BRIEF"
    turn "HANDOFF"
  elif [ "$line" = "FLOOD" ]; then
    n=1
    while [ $n -le @FLOOD_LINES@ ]; do
      printf 'CFSMOKE-FLOOD %s %s\n' "$n" "$pad"
      n=$((n + 1))
    done
    printf 'CFSMOKE-FLOODED %s\n' "$CFSMOKE_TAG"
  else
    # A paste arrives bracketed; the record and the hex are of the text.
    line=${line#"$esc[200~"}
    line=${line%"$esc[201~"}
    # Shell builtins only, on purpose. A chief pane's PATH once carried just
    # ConsensFlow's own bin directories — this fixture is what found that,
    # by failing on a missing od — and it is fixed now.
    # Keeping the hex in the shell means this test measures the app, not the
    # machine's coreutils.
    hex=''
    json=''
    rest=$line
    while [ -n "$rest" ]; do
      ch=${rest%"${rest#?}"}
      # Bytes above 0x7f come back sign-extended from printf; keep the byte.
      hex="$hex$(printf '%02x' $(( $(printf '%d' "'$ch") & 255 )))"
      case "$ch" in
        \\) json="$json\\\\" ;;
        \") json="$json\\\"" ;;
        *) json="$json$ch" ;;
      esac
      rest=${rest#?}
    done
    turn "$json"
    printf 'CFSMOKE-HEX %s\n' "$hex"
  fi
done
