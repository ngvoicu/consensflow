//! The eval matrix on the machine: each scenario for each chief and staff pair, one
//! run after another, each run's output kept in `evals/reports/windows/<time>/`
//! here (the run's own report stays on the machine), and one line per run at the
//! end: `PASS` when every check passed, else the checks that failed. The status is
//! 1 when any run did not pass, and 130 when the matrix was stopped.
//!
//! A run is [`run`](super::run::run) on the machine, the eval started there as
//! `cargo xtask eval ...` in the copy (`--build` builds the copy before the first
//! run that goes). What the eval says is read for its checks (`PASS …` and `FAIL …`
//! lines) and for the daemon it says it ran against (`daemon: native (rust 3.0.0)`,
//! which the line of its run ends with). A run that could not even start, or whose
//! output has no check in it, is a line of no verdict, which says the status the
//! run ended with (the command's own, as the machine said it) and where its log
//! is; the reason of one that could not start is in its log.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use cf_base::time::{iso, Clock};

use super::interrupt::Interrupt;
use super::options::{MatrixOptions, Options, Pair, DEFAULT_DIR};
use super::run::{self, Outcome};
use super::{Error, STOPPED};
use crate::context::Context;
use crate::dispatch::Console;

/// How long each eval may take, in minutes, as the eval is told.
const TIMEOUT_MIN: &str = "30";

/// What the matrix runs its evals on.
pub trait Remote {
    /// One run on the machine: what it says goes to `console`.
    fn run(&mut self, options: &Options, console: &mut Console) -> Result<Outcome, Error>;
}

/// The machine, as `windows` reaches it.
pub struct Machine<'a> {
    pub context: &'a Context,
    pub interrupt: &'a Interrupt,
}

impl Remote for Machine<'_> {
    fn run(&mut self, options: &Options, console: &mut Console) -> Result<Outcome, Error> {
        run::run(self.context, options, console, self.interrupt)
    }
}

/// What a run came to, in a line.
struct Verdict {
    ok: bool,
    line: String,
}

/// Runs every scenario for every pair on `remote`, in the order named, and says what each came to.
pub fn run(
    context: &Context,
    options: &MatrixOptions,
    remote: &mut dyn Remote,
    clock: &mut dyn Clock,
    console: &mut Console,
) -> Result<i32, Error> {
    let folder = logs_in(&context.root, clock);
    fs::create_dir_all(&folder).map_err(Error::files("make", &folder))?;
    let mut results: Vec<(String, Verdict)> = Vec::new();
    // Built once, before the first run that goes.
    let mut build = options.build;
    for scenario in &options.scenarios {
        for pair in &options.pairs {
            let name = format!("{scenario} {}:{}", pair.chief, pair.staff);
            if scenario == "chief-switch" && pair.chief == pair.staff {
                let line = "skipped: a chief switch needs another harness on the staff";
                results.push((
                    name,
                    Verdict {
                        ok: true,
                        line: line.into(),
                    },
                ));
                continue;
            }
            writeln!(console.out, "{name}…").map_err(Error::Output)?;
            let log = folder.join(format!("{scenario}-{}-{}.log", pair.chief, pair.staff));
            let run = Options {
                host: options.host.clone(),
                build,
                dir: DEFAULT_DIR.to_owned(),
                inside: ".".to_owned(),
                env: options.env.clone(),
                command: eval(options, scenario, pair),
            };
            build = false;
            let Some(verdict) = once(remote, &run, &log)? else {
                return Ok(STOPPED);
            };
            writeln!(console.out, "  {}", verdict.line).map_err(Error::Output)?;
            results.push((name, verdict));
        }
    }
    writeln!(console.out, "\nlogs: {}", folder.display()).map_err(Error::Output)?;
    for (name, verdict) in &results {
        writeln!(console.out, "{name:<36} {}", verdict.line).map_err(Error::Output)?;
    }
    Ok(i32::from(!results.iter().all(|(_, verdict)| verdict.ok)))
}

/// Where the logs of a matrix begun now are kept: the time, as the folder's name.
fn logs_in(root: &Path, clock: &mut dyn Clock) -> PathBuf {
    let now = iso(clock.now_ms());
    let stamp = now.get(..19).unwrap_or(&now).replace(':', "-");
    ["evals", "reports", "windows", stamp.as_str()]
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

/// The eval of `scenario` for `pair`, as the words that start it on the machine.
fn eval(options: &MatrixOptions, scenario: &str, pair: &Pair) -> Vec<String> {
    let mut words: Vec<String> = [
        "cargo",
        "xtask",
        "eval",
        "--scenario",
        scenario,
        "--chief",
        &pair.chief,
        "--staff",
        &pair.staff,
        "--timeout-min",
        TIMEOUT_MIN,
    ]
    .map(str::to_owned)
    .into();
    // A chief switch goes to the staff's harness.
    if scenario == "chief-switch" {
        words.extend(["--switch-to".to_owned(), pair.staff.clone()]);
    }
    if let Some(model) = options
        .claude_model
        .as_ref()
        .filter(|_| pair.chief == "claude")
    {
        words.extend(["--model".to_owned(), model.clone()]);
    }
    // A Pi member's model, on the machine's own signed-in provider (zeewin: openai/gpt-5.6-luna).
    if let Some(model) = options
        .pi_model
        .as_ref()
        .filter(|_| pair.staff.split(',').any(|staff| staff == "pi"))
    {
        words.extend(["--pi-staff-model".to_owned(), model.clone()]);
    }
    words
}

/// What one run said, as it said it: kept in the log and in memory, both of its
/// streams in the order they came.
struct Log {
    file: File,
    text: Vec<u8>,
}

/// One of the two streams of a run, written to its log.
struct Said<'a>(&'a RefCell<Log>);

impl Write for Said<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut log = self.0.borrow_mut();
        log.file.write_all(bytes)?;
        log.text.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.borrow_mut().file.flush()
    }
}

/// One run on `remote`, its output kept in `log`: what it came to, or none if
/// this program was stopped.
fn once(remote: &mut dyn Remote, run: &Options, log: &Path) -> Result<Option<Verdict>, Error> {
    let file = File::create(log).map_err(Error::files("make", log))?;
    let said = RefCell::new(Log {
        file,
        text: Vec::new(),
    });
    let ended = {
        let (mut out, mut err) = (Said(&said), Said(&said));
        remote.run(
            run,
            &mut Console {
                out: &mut out,
                err: &mut err,
            },
        )
    };
    let Log { mut file, text } = said.into_inner();
    let (status, extra) = match ended {
        Ok(Outcome::Stopped) => return Ok(None),
        Ok(Outcome::Exited(status)) => (status, String::new()),
        // It could not start, or went wrong on the way: the reason is for the log.
        Err(error) => {
            let reason = format!("windows: {error}\n");
            file.write_all(reason.as_bytes())
                .map_err(Error::files("write", log))?;
            (1, reason)
        }
    };
    let text = String::from_utf8_lossy(&text).into_owned() + &extra;
    Ok(Some(verdict(&text, status, log)))
}

/// What an eval's output says it came to: its checks (`PASS …`, `FAIL …`), and
/// the daemon it ran against.
fn verdict(text: &str, status: i32, log: &Path) -> Verdict {
    let (mut checks, mut failed, mut daemon) = (0, Vec::new(), None);
    for line in text.lines() {
        if let Some(said) = line.trim_start().strip_prefix("FAIL ") {
            checks += 1;
            if !said.is_empty() {
                failed.push(said);
            }
        } else if line.trim_start().starts_with("PASS ") {
            checks += 1;
        }
        if daemon.is_none() {
            daemon = line
                .strip_prefix("daemon: ")
                .filter(|said| !said.is_empty());
        }
    }
    let on = daemon.map_or_else(String::new, |daemon| format!(" · daemon {daemon}"));
    if checks == 0 {
        return Verdict {
            ok: false,
            line: format!("no verdict (exit {status}): see {}{on}", log.display()),
        };
    }
    if failed.is_empty() {
        Verdict {
            ok: true,
            line: format!("PASS {checks} checks{on}"),
        }
    } else {
        Verdict {
            ok: false,
            line: format!("FAIL {}{on}", failed.join(" · ")),
        }
    }
}

#[cfg(test)]
mod tests;
