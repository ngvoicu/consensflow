# The scripts `cargo xtask windows` sends, as recorded

Seven runs, which `a_run_is_sent_the_script_that_was_recorded_for_it_byte_for_byte`
(`tools/xtask/src/windows/script/tests/golden.rs`) names: six that name no variable
and one, `env-with-the-build`, that names two. A file is the whole script as ssh
is handed it, before it is encoded. Since the run script moved into the tarball (T-263) a
run has two: `<name>.ps1`, the bootstrap that goes on ssh's command line (the lock, the
unpack, the sweep, the tree said), and `<name>.run.ps1`, the run script it reads from the
tarball (the steps, the variables, the command), fourteen files in all.

They were first recorded on 2026-10-12 from `remoteScript` of the JavaScript
runner (`tests/live/windows-script.mjs` at 80dbeb5b), which the Rust one
replaced, and held to it byte for byte. The script has since gained its last
line (`== exit N`, written by the function `Exit-Run`, which every way out of
the script goes through: see the module doc of `windows`), and the files were
recorded again from the template in `script.rs`. The unpack has since gained `-m`
(`tar.exe -xmzf`: each file has the time of the unpack, not the one the Mac kept of
it, so that cargo there builds the file a switch of the checkout changed even where
its time is older than the last build), and the files were recorded again. It has since
been given its tarball by the run (`<folder>.<id>.tgz`, which the tests name
`0000abcd-12345678`; the lock holds the same name), a `finally` that removes it, and the
line that says which tree it unpacked (`== tree …`), and the files were recorded again. The
unpack has since run Windows' own tar by its path (`& $tar`, `System32\tar.exe`, and the run
ends saying so where there is none), as a bare `tar.exe` gave Git's GNU tar on CI's PATH, and
the files were recorded again. The
JavaScript is gone, so a
golden is no longer a proof that the script is as it was: it holds what the
script is, so that an edit nobody meant to it shows.

A file has one thing changed from the script: where the script has a carriage
return, the file has `␍` (U+240D), and the tests put it back. There are two in
each run script (`.run.ps1`), and they are on purpose: the batch file the command goes to is written
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
