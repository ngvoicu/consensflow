<!-- The paragraphs of the role texts that depend on the shell a window's commands are typed in
(T-1435): one part for each shell and slot, marked as staff.md marks its parts with the shell in
place of the role. chief.md and staff.md hold the slot as {{slot}}; the shell is the one the
window's cf word is for (cf_proto::cf_word::Shell): posix is bash, zsh and Git Bash, powershell is
Codex's and Muse's on Windows. A part holds {{cf}} as the other texts do. PowerShell has no
heredoc and reads $, the backtick and a quote its own way, so how a text is passed to cf, and
how a removal whose path a variable makes is written, are written for each shell. -->
<!-- A part marked `mac` is for the platform, not a shell: the paragraph a window on a Mac is given. -->
<!-- mac: home -->
Look for files inside the project folder, never over the home folder or in `~/Music` or `~/Pictures` (`find ~`, `grep -r ~`, `ls ~/Music`): macOS then asks the person at the screen, in ConsensFlow's name, and your command waits until they answer.
<!-- posix: chief_input -->
Any "…" can be `-`: the text then comes from standard input, as written. Pass a brief that holds backticks, `$` or quotes in a heredoc whose word is quoted, `<<'BRIEF'`, always: in double quotes or an unquoted heredoc the shell runs backticks and `$( )` as commands before ConsensFlow sees them. To put a value in, write it out.

    {{cf}} task add --tier standard - <<'BRIEF'
    The brief, as long as it needs, `code` and all.
    BRIEF

Write a long brief to a file with your file tool and pass it with `--file brief.md` instead: a shell can cut a long heredoc.
<!-- powershell: chief_input -->
Any "…" can be `--file brief.md`: the text then comes from that file, as written. Write a brief that holds backticks, `$` or quotes to it with your file tool, never with `>` (Windows PowerShell 5.1 writes UTF-16) or a pipe into `-` (it loses non-ASCII there). In double quotes PowerShell evaluates `$( )`, expands `$name` and treats a backtick as an escape before ConsensFlow sees the text, and 5.1 splits it at an inner `"`; to put a value in, write it out:

    {{cf}} task add --tier standard --file brief.md
<!-- posix: member_input -->
Any "…" can be `-`, with the text in a quoted heredoc, taken as written (in double quotes the shell runs backticks and `$( )`): `{{cf}} ask - <<'TEXT'`, the text, then a line `TEXT`.
<!-- powershell: member_input -->
Any "…" can be `--file ask.md`, the text in that file, written with your file tool and taken as written: `{{cf}} ask --file ask.md`. In double quotes PowerShell evaluates `$( )` and expands `$name`; a backtick escapes.
<!-- posix: removal -->
Nobody watches this window: Claude Code stops for a human on any removal (`rm`, `rm -f`, `rmdir`) whose path a variable makes (`"$R/$b"`), whatever the permissions. Write the paths out in full, or as `"${R:?}/${b:?}"`.
<!-- powershell: removal -->
Nobody watches this window: Claude Code stops for a human on any removal (`rm`, `rm -f`, `rmdir`) whose path a variable makes, whatever the permissions. Write the paths out in full.
