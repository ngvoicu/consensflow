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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use cf_base::args::{Opt, Positionals};
use cf_base::env::Env;
use cf_process::{execute, on_path, runnable, Limits};
use cf_proto::agents::is_parent_session_variable;
use cf_proto::page::{HandleLine, Refused, RefusedLine};

use crate::errors::install_hook;
use crate::files::SETTLE;
use crate::start::{start, OnOut, Options};

/// What the verb's arguments asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Flags {
    json: bool,
    no_open: bool,
}

/// `cf ui`: runs the daemon of this process, to the end of it. It returns only
/// when the daemon could not start, with the status that says why (1, or 74 for a
/// ledger that cannot be saved to), the reason said by [`refuse`]; a daemon that
/// started ends the process itself.
///
/// It must run before the standard streams' locks are taken: the daemon reads
/// its input and writes its output from other threads, which would wait for a
/// held lock for good.
pub fn ui(env: &Env, args: &[OsString]) -> i32 {
    ui_with(env, args, serve)
}

/// `ui`, with `serve` the daemon it hands to its thread (the tests give it another).
fn ui_with(env: &Env, args: &[OsString], serve: fn(&Env, &[OsString]) -> i32) -> i32 {
    let args_text = texts(args);
    let (env, args) = (env.clone(), args.to_vec());
    match on_a_stack_of_its_own(move || serve(&env, &args)) {
        Ok(code) => code,
        Err(failed) => refuse(
            asked_for_json(&args_text),
            1,
            &format!("could not start its thread: {failed}"),
        ),
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
    let words = texts(args);
    let json = asked_for_json(&words);
    let flags = match parse(&words) {
        Ok(flags) => flags,
        Err(words) => return refuse(json, 1, &words),
    };
    install_hook();
    // Once the handle line is said the output is the bridge's: nothing more is said on it.
    let handed = Arc::new(AtomicBool::new(false));
    let options = match Options::process(on_out(flags, env, Arc::clone(&handed))) {
        Ok(options) => options,
        Err(failed) => return refuse(json, 1, &failed.to_string()),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(failed) => return refuse(json, 1, &format!("could not start its runtime: {failed}")),
    };
    let local = tokio::task::LocalSet::new();
    let outcome = local.block_on(&runtime, async {
        match start(env.clone(), options).await {
            Ok(daemon) => {
                daemon.finished().await;
                Ok(())
            }
            Err(failed) => Err(failed),
        }
    });
    // Neither is let wait for the read of the input, which cannot be
    // cancelled: a daemon that stopped has exited, and one that never started
    // has no such read.
    drop(local);
    runtime.shutdown_background();
    match outcome {
        Ok(()) => 0,
        Err(failed) => refuse(
            json && !handed.load(Ordering::SeqCst),
            failed.exit_code(),
            &failed.to_string(),
        ),
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

/// Says a refusal of `cf ui` and gives the status the process ends with, whatever became of
/// the saying: under `--json` the refusal line goes to the output, in the handle line's place,
/// and then the words to the error output, as `cf: <words>`. A thread says it and is waited
/// for [`SETTLE`] at most; one that cannot be made, or is still writing (a reader that drains
/// nothing), loses the words, never the end.
fn refuse(json: bool, exit: i32, words: &str) -> i32 {
    let words = words.to_owned();
    let line = json
        .then(|| {
            serde_json::to_string(&RefusedLine {
                refused: Refused {
                    exit,
                    pid: std::process::id(),
                    words: words.clone(),
                },
            })
            .ok()
        })
        .flatten();
    let (done, heard) = mpsc::channel();
    let saying = std::thread::Builder::new()
        .name("consensflow-refusal".to_owned())
        .spawn(move || {
            if let Some(line) = line {
                let mut out = io::stdout().lock();
                let _ = writeln!(out, "{line}").and_then(|()| out.flush());
            }
            let _ = writeln!(io::stderr(), "cf: {words}");
            let _ = done.send(());
        });
    if saying.is_ok() {
        let _ = heard.recv_timeout(SETTLE);
    }
    exit
}

/// The arguments as text.
fn texts(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// Whether the words ask for `--json`, read where the arguments are themselves refused.
fn asked_for_json(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json" || arg.starts_with("--json="))
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
fn on_out(flags: Flags, env: &Env, handed: Arc<AtomicBool>) -> OnOut {
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
        handed.store(true, Ordering::SeqCst);
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
