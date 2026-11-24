//! `cf ui [--json] [--no-open]`: the verb that runs the daemon, as Node's CLI
//! ran it (with the drain for a broken pipe).
//!
//! `--json` prints the handle line, the first line of the output, which the
//! app reads. A person gets the address of the harnesses screen instead, and the
//! screen opened with `open` unless `--no-open` is given. Either way the
//! bridge goes on over the same standard output, and the daemon stops when its
//! input ends, when it is signalled, or when nobody reads its output or its
//! error output any more.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use cf_base::args::{Opt, Positionals};
use cf_base::env::Env;
use cf_process::{execute, on_path, runnable, Limits};
use cf_proto::agents::is_parent_session_variable;
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
    ui_with(env, args, serve)
}

/// `ui`, with `serve` the daemon it hands to its thread (the tests give it another).
fn ui_with(env: &Env, args: &[OsString], serve: fn(&Env, &[OsString]) -> i32) -> i32 {
    let (env, args) = (env.clone(), args.to_vec());
    match on_a_stack_of_its_own(move || serve(&env, &args)) {
        Ok(code) => code,
        Err(failed) => fail(&format!("could not start its thread: {failed}")),
    }
}

/// The stack of the thread the daemon runs on: 16 MiB. A process's main thread has the system's
/// default, which is 1 MiB on Windows (8 MiB on Linux and macOS), and the calls a launch nests
/// in a debug build take more than 1 MiB there: the daemon overflowed its stack on Windows when
/// it was asked for a chief's window, once a launch asked its harness its version first
/// (T-1360, found by CI's `daemon_stop` and `background_close`).
pub(crate) const DAEMON_STACK_BYTES: usize = 16 * 1024 * 1024;

/// What the daemon's thread is called.
pub(crate) const DAEMON_THREAD: &str = "consensflow-daemon";

/// `work` on a thread of its own with [`DAEMON_STACK_BYTES`] of stack, waited for: what it
/// returns, or its panic carried on to this thread as it would have been on its own.
pub(crate) fn on_a_stack_of_its_own<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<T> {
    let thread = std::thread::Builder::new()
        .name(DAEMON_THREAD.to_owned())
        .stack_size(DAEMON_STACK_BYTES)
        .spawn(work)?;
    Ok(thread
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
}

/// `ui` on the thread it was given.
fn serve(env: &Env, args: &[OsString]) -> i32 {
    let env = &outside_parent_sessions(env);
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

/// `env` without a session `cf ui` was started in ([`is_parent_session_variable`]), as the
/// app takes them out of its own: run in an agent's shell or a terminal's pane, the daemon
/// would hand that session to every program it starts of its own (a harness's probes, git,
/// a harness's throwaway server), which would be taken for the session's (T-1024). Its
/// windows are given none of it by the pane host.
fn outside_parent_sessions(env: &Env) -> Env {
    env.clone().without_vars(is_parent_session_variable)
}

/// What the verb says when it cannot go on, in `cf`'s words.
fn fail(words: &str) -> i32 {
    let _ = writeln!(io::stderr(), "cf: {words}");
    1
}

/// The verb's options as `parseArgs` of Node reads them (`cf_base::args`):
/// `--json` and `--no-open`, and every other word a positional that is
/// ignored. What Node refuses it refuses in its own words.
fn parse(args: &[String]) -> Result<Flags, String> {
    const OPTIONS: [Opt; 2] = [Opt::flag("json"), Opt::flag("no-open")];
    let parsed = cf_base::args::parse(args, &OPTIONS, Positionals::Allowed)?;
    Ok(Flags {
        json: parsed.flag("json"),
        no_open: parsed.flag("no-open"),
    })
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

/// Where a person opens the harnesses screen: the token goes in the query.
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
    // `open` is a program and no shim, so there is always a way to start it.
    let Ok(run) = runnable(&program, &[OsString::from(address)], env) else {
        return;
    };
    let env = env.clone();
    drop(tokio::task::spawn_local(async move {
        let limits = Limits {
            timeout: Duration::from_secs(30),
            max_buffer: 64 * 1024,
        };
        // Nothing waits for it, nor ends it with the daemon: it is `open`.
        let _ = execute(&run, None, &env, limits, |_| {}).await;
    }));
}

#[cfg(test)]
mod tests;
