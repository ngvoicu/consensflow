# The scripts `cargo xtask windows` sends, as the JavaScript runner sent them

Recorded on 2026-10-12 from `remoteScript` of `tests/live/windows-script.mjs`
(at 80dbeb5b, the last commit of the runner in JavaScript), for the seven runs
that `a_run_is_sent_the_script_the_javascript_sent_byte_for_byte`
(`tools/xtask/src/windows/script/tests.rs`) names. The first six name no
variable: they are also what `sentBefore` made, the copy of the script as it was
before `--env` existed that the old test held byte for byte (the recording
checked each equal to it). The seventh, `env-with-the-build`, names two.

A file is the whole script as ssh was handed it, with one thing changed: where
the script has a carriage return, the file has `␍` (U+240D), and the tests put
it back. There are two in each, and they are on purpose: the batch file the
command goes to is written with `"@echo off<CR><LF>"` and `"<CR><LF>"`, the line
ends being in the PowerShell string and not an escape PowerShell reads. Git's
`text=auto` would take a real carriage return out of a file, and the tests would
then be held to a script nobody sent.

A change to the script that is meant is made to the template (`script.rs`) and
to these files together; one that is not meant is what these files are for.
