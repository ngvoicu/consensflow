#!/bin/sh
# The stand-in harness of the packaged smoke's hidden-app case: a window that
# prints a flood while the app's window is hidden. It says it is ready, as the
# smoke's other harness does, so the page shows its window; then it waits for the
# file the smoke makes once the app is hidden, prints the flood to its terminal,
# and counts the lines it has printed in a file of its own, the last count once
# it is done. A window whose output the pane host stops reading waits in its
# terminal's write, and its count stops with it.
LC_ALL=C
export LC_ALL
# The daemon's read of the models Claude Code lists (`claude -p --input-format stream-json`: no
# window, no turn) is answered as the smoke's other harness answers it, with the models the page's
# self-test opens its project with, and ends with the input; it draws nothing and notes no pid of
# a window. The daemon takes no model its harness does not list (A4-28, T-1329) and waits 30 s for
# this answer, so a script without it has the project refused before the window opens (T-1362).
if [ "$1" = "-p" ]; then
  while IFS= read -r line; do
    case "$line" in
      *'"subtype":"initialize"'*)
        id=${line#*'"request_id":"'}
        id=${id%%'"'*}
        printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"models":[' "$id"
        sep=''
        for model in fake fake-2 fake-chief chief worker claude-fable-5-1 terpsichore; do
          printf '%s{"value":"%s","displayName":"%s","resolvedModel":"%s","supportsEffort":false}' "$sep" "$model" "$model" "$model"
          sep=','
        done
        printf ']}}}\n'
        ;;
    esac
  done
  exit 0
fi
echo $$ >> "$CFSMOKE_PIDFILE"
printf 'CFSMOKE-READY %s\n' "$CFSMOKE_TAG"
# The page shows the window once it has read both lines the smoke's other harness prints.
printf 'CFSMOKE-TOOLS ok\n'
pad=''
n=0
while [ $n -lt 118 ]; do
  pad="${pad}x"
  n=$((n + 1))
done
while [ ! -f "$CFSMOKE_GO" ]; do sleep 0.2; done
n=0
while [ $n -lt "$CFSMOKE_FLOOD_LINES" ]; do
  printf 'CFSMOKE-HIDDEN %07d %s\n' "$n" "$pad"
  n=$((n + 1))
  case $n in *000) echo "$n" > "$CFSMOKE_COUNT" ;; esac
done
echo "$n done" > "$CFSMOKE_COUNT"
exec sleep 3600
