# The scripts `cargo xtask windows` sends, as recorded

Seven runs, which `a_run_is_sent_the_script_that_was_recorded_for_it_byte_for_byte`
(`tools/xtask/src/windows/script/tests.rs`) names: six that name no variable
and one, `env-with-the-build`, that names two. A file is the whole script as ssh
is handed it, before it is encoded.

They were first recorded on 2026-10-12 from `remoteScript` of the JavaScript
runner (`tests/live/windows-script.mjs` at 80dbeb5b), which the Rust one
replaced, and held to it byte for byte. The script has since gained its last
line (`== exit N`, written by the function `Exit-Run`, which every way out of
the script goes through: see the module doc of `windows`), and the files were
recorded again from the template in `script.rs`. The JavaScript is gone, so a
golden is no longer a proof that the script is as it was: it holds what the
script is, so that an edit nobody meant to it shows.

A file has one thing changed from the script: where the script has a carriage
return, the file has `␍` (U+240D), and the tests put it back. There are two in
each, and they are on purpose: the batch file the command goes to is written
with `"@echo off<CR><LF>"` and `"<CR><LF>"`, the line ends being in the
PowerShell string and not an escape PowerShell reads. Git's `text=auto` would
take a real carriage return out of a file, and the tests would then be held to a
script nobody sent.

A change to the script that is meant is made to the template, and the files are
recorded again by the test that writes them:

```
cargo test -p xtask --lib windows::script::tests::golden::record_the_goldens -- --ignored
```

The diff of the files is the review of the change. One that is not meant is
what these files are for.
