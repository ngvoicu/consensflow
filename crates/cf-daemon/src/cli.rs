//! `cf ui [--json] [--no-open]`: the verb that runs the daemon (`bin/cf.mjs`,
//! `ui` at `:280-305` and the broken-pipe drain at `:15-28`).
//!
//! `--json` prints the handle line, the first line of the output, which the
//! app reads. A person gets the address of the agents screens instead, and the
//! screens opened with `open` unless `--no-open` is given. Either way the
//! bridge goes on over the same standard output, and the daemon stops when its
//! input ends, when it is signalled, or when nobody reads its output or its
//! error output any more.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{execute, on_path, runnable, Limits};
use cf_proto::page::HandleLine;

use crate::errors::install_hook;
use crate::start::{start, OnOut, Options};

/// What the verb's arguments asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    json: bool,
    no_open: bool,
}

/// `cf ui`: runs the daemon of this process, to the end of it. It returns only
/// when the daemon could not start (the code is 1, and the reason is on the
/// error output as `cf: <reason>`); a daemon that started ends the process
/// itself.
///
/// It must run before the standard streams' locks are taken: the daemon reads
/// its input and writes its output from other threads, which would wait for a
/// held lock for good.
pub fn ui(env: &Env, args: &[OsString]) -> i32 {
    let words: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let flags = match parse(&words) {
        Ok(flags) => flags,
        Err(words) => return fail(&words),
    };
    install_hook();
    let options = match Options::process(on_out(flags, env)) {
        Ok(options) => options,
        Err(failed) => return fail(&failed.to_string()),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(failed) => return fail(&format!("could not start its runtime: {failed}")),
    };
    let local = tokio::task::LocalSet::new();
    let outcome = local.block_on(&runtime, async {
        match start(env.clone(), options).await {
            Ok(daemon) => {
                daemon.finished().await;
                Ok(())
            }
            Err(failed) => Err(failed.to_string()),
        }
    });
    // Neither is let wait for the read of the input, which cannot be
    // cancelled: a daemon that stopped has exited, and one that never started
    // has no such read.
    drop(local);
    runtime.shutdown_background();
    match outcome {
        Ok(()) => 0,
        Err(words) => fail(&words),
    }
}

/// What the verb says when it cannot go on, in `cf`'s words.
fn fail(words: &str) -> i32 {
    let _ = writeln!(io::stderr(), "cf: {words}");
    1
}

/// The verb's options as `parseArgs` of Node reads them, strictly, positionals
/// allowed: `--json` and `--no-open`, a `--` that ends the options, and every
/// other word a positional that is ignored. What else Node refuses it refuses
/// in the words Node 26.8.1 used.
fn parse(args: &[String]) -> Result<Flags, String> {
    let mut flags = Flags::default();
    for arg in args {
        if arg == "--" {
            break;
        }
        if arg == "-" || !arg.starts_with('-') {
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            match name {
                "json" | "no-open" => {
                    if value.is_some() {
                        return Err(format!("Option '--{name}' does not take an argument"));
                    }
                    if name == "json" {
                        flags.json = true;
                    } else {
                        flags.no_open = true;
                    }
                }
                "" => return Err(unknown(arg)),
                _ => return Err(unknown(&format!("--{name}"))),
            }
        } else {
            // A short option: none is known, and the first letter is named.
            let letter: String = arg.chars().skip(1).take(1).collect();
            return Err(unknown(&format!("-{letter}")));
        }
    }
    Ok(flags)
}

/// Node's words for an option it does not know. Node's own sentence has no
/// closing quote after the option, and so has this.
fn unknown(option: &str) -> String {
    format!(
        "Unknown option '{option}'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- \"{option}\""
    )
}

/// What the verb prints for the handle line: the app's JSON, or a person's
/// address (the url, which ends in a slash, and the token) and a word on how
/// to stop.
fn lines(flags: Flags, handle: &HandleLine) -> io::Result<Vec<String>> {
    if flags.json {
        let line = serde_json::to_string(handle).map_err(io::Error::other)?;
        return Ok(vec![line]);
    }
    Ok(vec![
        format!("agents: {}", address(handle)),
        "Ctrl-C to stop — nothing keeps running after it.".to_owned(),
    ])
}

/// Where a person opens the agents screens: the token goes in the query.
fn address(handle: &HandleLine) -> String {
    format!("{}?token={}", handle.url, handle.token)
}

/// Says the handle line, and for a person opens the screens.
fn on_out(flags: Flags, env: &Env) -> OnOut {
    let env = env.clone();
    Box::new(move |handle| {
        {
            // The lock is held for these lines alone: the bridge writes the
            // same output from another thread as soon as this returns.
            let mut out = io::stdout().lock();
            for line in lines(flags, handle)? {
                writeln!(out, "{line}")?;
            }
            out.flush()?;
        }
        if !flags.json && !flags.no_open {
            open(&address(handle), &env);
        }
        Ok(())
    })
}

/// Opens `address` with the system's `open`, apart: the daemon does not wait
/// for it, nor mind how it ends (on a system with no `open` it does not start).
fn open(address: &str, env: &Env) {
    let program = on_path("open", env).unwrap_or_else(|| PathBuf::from("open"));
    let run = runnable(&program, &[OsString::from(address)], env);
    let env = env.clone();
    drop(tokio::task::spawn_local(async move {
        let limits = Limits {
            timeout: Duration::from_secs(30),
            max_buffer: 64 * 1024,
        };
        let _ = execute(&run, None, &env, limits).await;
    }));
}

#[cfg(test)]
mod tests;
