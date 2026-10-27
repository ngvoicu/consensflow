//! A stand-in for the programs xtask starts (`node`, `cargo`, `npm`), for its
//! tests: it says where it was started and with what, and ends as the
//! environment tells it.
//!
//! On its standard output, one line each: `cwd: <folder>`, then `arg: <word>` for
//! every argument (as Rust writes a string, so that a space, a quote or an empty
//! word shows), then, for each variable named in `FAKE_CHILD_REPORT` (comma
//! apart), `var <NAME>: <value>` or `var <NAME>: unset`. `FAKE_CHILD_LAST` is a
//! line it then writes, the last it says on its standard output (`== exit 0`, for
//! the runs on the Windows machine, which end by saying how the command ended), and
//! `FAKE_CHILD_FIRST` a line it writes before the rest (`== tree …`, which the machine
//! says of the tree it unpacked, once, before the command's own words).
//! With `FAKE_CHILD_KEEP` naming a folder it keeps a copy of any file it is given that is
//! named `….tgz` there (`1.tgz`, `2.tgz` in the order it was given them), which is what
//! `scp` is given: the tarball a run sends, for a test to read what is in it.
//! With `FAKE_CHILD_PROBE` set, an `ssh` asked as the runner probes a machine (a word
//! `BatchMode=yes` among its arguments) says that text, and nothing else, and ends with 0: the
//! machine's state (`free 614`), which the runner asks of every machine before it sends a run to
//! it, and which is no part of the run the case is about.
//! `FAKE_CHILD_STDERR` is written to its standard error, and `FAKE_CHILD_EXIT` is
//! the code it exits with (0 when it is not a number or not there). With
//! `FAKE_CHILD_QUIET` set it reports nothing: for a test that lets it write where
//! the test's own output goes.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::process::ExitCode;

use cf_base::env::Env;

fn main() -> ExitCode {
    let env = Env::from_process();
    if let Some(probe) = env.text("FAKE_CHILD_PROBE") {
        if std::env::args_os().any(|arg| arg == "BatchMode=yes") {
            let _ = writeln!(io::stdout().lock(), "{probe}");
            return ExitCode::SUCCESS;
        }
    }
    if let Some(keep) = env.path("FAKE_CHILD_KEEP") {
        for arg in std::env::args_os().skip(1) {
            let path = std::path::PathBuf::from(&arg);
            if path.extension().is_some_and(|ext| ext == "tgz") && path.is_file() {
                let kept = std::fs::read_dir(keep).map_or(0, Iterator::count) + 1;
                let _ = std::fs::copy(&path, keep.join(format!("{kept}.tgz")));
            }
        }
    }
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if let Some(first) = env.text("FAKE_CHILD_FIRST") {
        let _ = writeln!(out, "{first}");
    }
    if env.text("FAKE_CHILD_QUIET").is_none() {
        let folder = std::env::current_dir().map_or_else(
            |cause| format!("unknown ({cause})"),
            |dir| dir.display().to_string(),
        );
        let _ = writeln!(out, "cwd: {folder}");
        for arg in std::env::args_os().skip(1) {
            let _ = writeln!(out, "arg: {arg:?}");
        }
        let asked = env.text("FAKE_CHILD_REPORT").unwrap_or_default();
        for name in asked.split(',').filter(|name| !name.is_empty()) {
            match env.os(name) {
                Some(value) => {
                    let _ = writeln!(out, "var {name}: {value:?}");
                }
                None => {
                    let _ = writeln!(out, "var {name}: unset");
                }
            }
        }
    }
    if let Some(last) = env.text("FAKE_CHILD_LAST") {
        let _ = writeln!(out, "{last}");
    }
    let _ = out.flush();
    if let Some(text) = env.text("FAKE_CHILD_STDERR") {
        let _ = writeln!(io::stderr().lock(), "{text}");
    }
    match env
        .text("FAKE_CHILD_EXIT")
        .and_then(|code| code.parse::<i32>().ok())
    {
        Some(code) => u8::try_from(code).map_or_else(|_| std::process::exit(code), ExitCode::from),
        None => ExitCode::SUCCESS,
    }
}
