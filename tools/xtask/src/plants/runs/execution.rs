//! Execute a plant's command, manage its process and prepare its report.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::future::{poll_fn, Future};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{capture_whole, Limits, Run};
use tokio::runtime::Runtime;

use super::reporters::{caller_threads, Reporter};
use super::{Error, Execution, Ran, Runner, CF_E2E_PROGRAMS, NEXTEST_BUILD_FAILED, NONE_RAN};
use crate::context::Context;
use crate::process::{self, Invocation};
use crate::stops::Stops;

/// How much one stream of a run may say: far more than any run of tests does.
const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

/// The variable `node --test` sets for the ones it starts.
const NODE_TEST_CONTEXT: &str = "NODE_TEST_CONTEXT";

/// Carries the run's test-only temp folder through nextest, without changing Cargo's temp.
const TEST_TEMP: &str = "CF_PLANTS_PRIVATE_TEMP";

/// Where each run's report is written, under the build folder git leaves out.
const RUNS: &str = "app/src-tauri/target/plants-runs";

/// Where the workspace's tools are installed (`cargo install --root`).
const TOOLS: &str = "app/src-tauri/target/tools";

/// The variable that tells the tests of `cf-e2e` the folder the programs they run
/// were built in before the run (crates/cf-e2e/src/cf.rs, `BUILT`).
const CF_E2E_BUILT: &str = "CF_E2E_BUILT";

/// Runs the commands as processes, within a limit of time, through `cf-process`.
pub struct Execute {
    runtime: Runtime,
    /// The checkout, and the environment the commands are given.
    checkout: Context,
    own: PathBuf,
    pub(super) limits: Limits,
    stops: Stops,
    /// How many states could not be told, each told apart from every other.
    untold: u64,
    /// The builds before a run of cf-e2e's tests (`CF_E2E_PROGRAMS`).
    pub(super) programs: Vec<Vec<String>>,
    /// Whether a run that takes no threads was told the driver's threads have no
    /// effect on it: said once, and the run goes on.
    said_no_effect: bool,
    test_temp: Option<PathBuf>,
}

impl Execute {
    /// A runner of commands from the checkout's root, none given more than `limit`.
    /// `own` is this xtask, which `cargo xtask` is. The stops are listened for from here on.
    pub fn new(context: &Context, own: PathBuf, limit: Duration) -> Result<Self, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::files(
                "start the runtime to run the plants in",
                &context.root,
            ))?;
        let stops = {
            let _inside = runtime.enter();
            Stops::listen().map_err(Error::files("listen for a stop", &context.root))?
        };
        Ok(Self::with_stops(context, own, limit, runtime, stops))
    }

    /// As [`new`](Self::new), with the stops it is given and the runtime they were made in.
    pub(in crate::plants) fn with_stops(
        context: &Context,
        own: PathBuf,
        limit: Duration,
        runtime: Runtime,
        stops: Stops,
    ) -> Self {
        let colour = [OsStr::new("NO_COLOR"), OsStr::new("FORCE_COLOR")];
        let env = Env::from_vars(
            context
                .env
                .iter()
                .filter(|(name, _)| *name != OsStr::new(NODE_TEST_CONTEXT))
                // Only a run of cf-e2e's tests is told where its programs were built.
                .filter(|(name, _)| *name != OsStr::new(CF_E2E_BUILT))
                .filter(|(name, _)| !colour.contains(name))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain([
                    (OsString::from("NO_COLOR"), OsString::from("1")),
                    (OsString::from("FORCE_COLOR"), OsString::from("0")),
                ]),
        );
        Self {
            runtime,
            checkout: Context {
                root: context.root.clone(),
                env,
            },
            own,
            limits: Limits {
                timeout: limit,
                max_buffer: OUTPUT_LIMIT,
            },
            stops,
            untold: 0,
            programs: CF_E2E_PROGRAMS
                .iter()
                .map(|build| build.iter().map(|word| (*word).to_owned()).collect())
                .collect(),
            said_no_effect: false,
            test_temp: None,
        }
    }

    /// What `git` says to `words` in the checkout, where it says it and succeeds: the
    /// outer `None` is a stop, heard while it ran, which is the driver's to hear.
    fn git(&mut self, words: &[&str]) -> Option<Option<String>> {
        let command: Vec<String> = ["git"]
            .iter()
            .chain(words)
            .map(|word| (*word).to_owned())
            .collect();
        match self.run(&command) {
            Ok(Execution::Stopped) => None,
            Ok(Execution::Ended(ran)) if ran.code == Some(0) => Some(Some(ran.output)),
            _ => Some(None),
        }
    }

    /// A folder for a run's report, under `RUNS`, removed once dropped: one of its own
    /// for each run, whatever else runs beside it.
    fn folder(&self) -> Result<tempfile::TempDir, Error> {
        let runs = self.checkout.root.join(RUNS);
        fs::create_dir_all(&runs)
            .and_then(|()| tempfile::Builder::new().prefix("run-").tempdir_in(&runs))
            .map_err(Error::files("make a folder for a run's report in", &runs))
    }

    /// Where nextest is: installed for the workspace (`TOOLS`), or on the PATH.
    pub(super) fn nextest(&self) -> Result<String, Error> {
        let name = if cfg!(windows) {
            "cargo-nextest.exe"
        } else {
            "cargo-nextest"
        };
        let installed = self.checkout.root.join(TOOLS).join("bin").join(name);
        if installed.is_file() {
            return Ok(installed.display().to_string());
        }
        let found = process::resolved(OsStr::new("cargo-nextest"), &self.checkout.env);
        if Path::new(&found).components().count() > 1 {
            return Ok(found.to_string_lossy().into_owned());
        }
        Err(process::Failure::NotFound {
            program: format!(
                "cargo-nextest (a fresh tree copies it in: mkdir -p {TOOLS}/bin && cp ~/Projects/ngvoicu/consensflow-trees/.runs/tools/bin/cargo-nextest {TOOLS}/bin/)"
            ),
        }
        .into())
    }

    /// Builds the programs the tests of `cf-e2e` run (`CF_E2E_PROGRAMS`), before a run
    /// of `command`: the folder they are in, where cargo says it put each program a
    /// build names (`--bin`), whatever folder it builds in (`CARGO_TARGET_DIR`, its
    /// config); or, where a build did not succeed, how the run ends: stopped; the
    /// build itself, where its time ran out; else as `command` with the build's
    /// status and words and no test run, which a build cargo says failed is one that
    /// did not build. Builds that put the programs in two folders, or that say of
    /// none where they put it, are no run of the tests either.
    fn programs(&mut self, command: &[String]) -> Result<Result<PathBuf, Execution>, Error> {
        let mut folder: Option<PathBuf> = None;
        for words in self.programs.clone() {
            let ran = match self.run(&words)? {
                Execution::Stopped => return Ok(Err(Execution::Stopped)),
                Execution::Ended(ran) if ran.code == Some(0) => {
                    let bin = words.iter().skip_while(|word| *word != "--bin").nth(1);
                    let Some(bin) = bin else { continue };
                    let built = ran.executable(bin);
                    let Some(there) = built.as_deref().and_then(Path::parent) else {
                        let why = format!("{} said nowhere it put {bin}", words.join(" "));
                        return Ok(Err(Execution::Ended(Self::unbuilt(command, ran, why))));
                    };
                    if let Some(other) = folder.as_ref().filter(|other| *other != there) {
                        let why = format!(
                            "{} put {bin} in {}, and the build before it in {}",
                            words.join(" "),
                            there.display(),
                            other.display()
                        );
                        return Ok(Err(Execution::Ended(Self::unbuilt(command, ran, why))));
                    }
                    folder = Some(there.to_path_buf());
                    continue;
                }
                // Ended for its time or its output, it is said as the build it is.
                Execution::Ended(ran) if ran.hung || ran.overflowed => {
                    return Ok(Err(Execution::Ended(ran)))
                }
                Execution::Ended(ran) => ran,
            };
            let code = if ran.build_finished() == Some(false) {
                Some(NEXTEST_BUILD_FAILED)
            } else {
                ran.code
            };
            let why = format!("{} did not succeed", words.join(" "));
            return Ok(Err(Execution::Ended(Self::unbuilt(
                command,
                Ran { code, ..ran },
                why,
            ))));
        }
        Ok(match folder {
            Some(folder) => Ok(folder),
            None => Err(Execution::Ended(Ran {
                command: command.to_vec(),
                code: None,
                hung: false,
                overflowed: false,
                output: String::new(),
                report: Some(Err(format!(
                    "no build before it named a program it built{NONE_RAN}"
                ))),
            })),
        })
    }

    /// A run of `command` that ran no test, the build `ran` before it having come to
    /// `why`: its status, its words and its end are the build's.
    fn unbuilt(command: &[String], ran: Ran, why: String) -> Ran {
        Ran {
            command: command.to_vec(),
            report: Some(Err(format!("{why}{NONE_RAN}"))),
            ..ran
        }
    }

    /// The program to start for `invocation`, and its words: `cargo xtask` is this xtask.
    fn start(&self, invocation: &Invocation) -> Result<Run, Error> {
        let (program, args): (OsString, &[OsString]) = match invocation.args.as_slice() {
            [xtask, args @ ..] if invocation.program == "cargo" && xtask == "xtask" => {
                (self.own.clone().into_os_string(), args)
            }
            args => (
                process::resolved(&invocation.program, &self.checkout.env),
                args,
            ),
        };
        // A bare name that the PATH did not give is a program that is not there.
        if Path::new(&program).components().count() <= 1 {
            return Err(process::Failure::NotFound {
                program: program.to_string_lossy().into_owned(),
            }
            .into());
        }
        Ok(Run {
            program: PathBuf::from(program),
            args: args.to_vec(),
            verbatim: false,
        })
    }
}

impl Runner for Execute {
    fn lacks(&mut self, harness: &str) -> Option<String> {
        super::super::kit::lacks(harness, self.checkout.env.path("HOME"))
    }

    fn temp_in(&mut self, folder: &Path) {
        self.test_temp = Some(folder.to_path_buf());
    }

    fn say_no_effect(&mut self, command: &[String]) {
        // A plants table's own [run] command that takes no threads: told once theirs
        // has no effect on it, and the run goes on.
        #[allow(clippy::print_stderr)] // The runner has no console of its own.
        if !self.said_no_effect && !Reporter::of(command).takes_threads(command) {
            if let Some(threads) = caller_threads(&self.checkout) {
                eprintln!(
                    "plants: --test-threads {threads} has no effect on: {}",
                    command.join(" ")
                );
                self.said_no_effect = true;
            }
        }
    }

    fn run(&mut self, command: &[String]) -> Result<Execution, Error> {
        let reporter = Reporter::of(command);
        if let Some(why) = reporter.refused(command, &self.checkout) {
            return Err(process::Failure::Refused {
                program: command.join(" "),
                cause: std::io::Error::other(why),
            }
            .into());
        }
        // The programs the tests of cf-e2e run, built once for the run, before it: each
        // of its tests is a process of its own, which would build them for itself.
        let built = if reporter.runs_cf_e2e(command, &self.checkout) {
            match self.programs(command)? {
                Ok(folder) => Some(folder),
                Err(ended) => return Ok(ended),
            }
        } else {
            None
        };
        // A folder of the run's own, gone with it, for a reporter that writes one.
        let folder = match reporter {
            Reporter::Status => None,
            _ => Some(self.folder()?),
        };
        let dir = folder.as_ref().map_or_else(
            || self.checkout.root.clone(),
            |folder| folder.path().to_path_buf(),
        );
        let dir = dir.as_path();
        let nextest = match reporter {
            Reporter::Nextest => self.nextest()?,
            _ => String::new(),
        };
        let mut invocation = reporter
            .setup(command, dir, &self.checkout, &nextest)
            .map_err(Error::files("write the config of a run in", dir))?;
        if let Some(temp) = &self.test_temp {
            if reporter == Reporter::Nextest {
                // Nextest invokes Cargo first, then runs setup scripts and tests. The
                // setup script supplies only the tests' environment, without wrapping
                // their executables: a binary nextest cannot start must remain an error,
                // never a test failure that could falsely catch a plant.
                let config = dir.join("nextest.toml");
                let original = fs::read_to_string(&config)
                    .map_err(Error::files("read nextest's config", &config))?;
                let setup = if cfg!(windows) {
                    r#"command = ['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', '[IO.File]::WriteAllText($env:NEXTEST_ENV, ("TMPDIR={0}`nTMP={0}`nTEMP={0}`n" -f $env:CF_PLANTS_PRIVATE_TEMP), [Text.UTF8Encoding]::new($false))']"#
                } else {
                    r#"command = ['/bin/sh', '-c', 'printf "TMPDIR=%s\nTMP=%s\nTEMP=%s\n" "$CF_PLANTS_PRIVATE_TEMP" "$CF_PLANTS_PRIVATE_TEMP" "$CF_PLANTS_PRIVATE_TEMP" > "$NEXTEST_ENV"']"#
                };
                // Listing also starts the binary (and any constructors) before
                // setup. Wrap listing alone: if it cannot start, nextest refuses
                // the whole run. Actual tests must run directly so an unstartable
                // test is never mistaken for a failed assertion.
                let list = if cfg!(windows) {
                    let script = dir.join("plants-list.ps1");
                    fs::write(
                        &script,
                        r#"$env:TMPDIR=$env:CF_PLANTS_PRIVATE_TEMP
$env:TMP=$env:CF_PLANTS_PRIVATE_TEMP
$env:TEMP=$env:CF_PLANTS_PRIVATE_TEMP
if ($args.Count -lt 1) { exit 127 }
$program=$args[0]
$remaining=@($args | Select-Object -Skip 1)
& $program @remaining
exit $LASTEXITCODE
"#,
                    )
                    .map_err(Error::files("write nextest's list wrapper in", &script))?;
                    let path = serde_json::to_string(&script.to_string_lossy())
                        .map_err(std::io::Error::other)
                        .map_err(Error::files("encode nextest's list wrapper", &script))?;
                    format!("command = ['powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', {path}]")
                } else {
                    r#"command = ['/bin/sh', '-c', 'export TMPDIR="$CF_PLANTS_PRIVATE_TEMP" TMP="$CF_PLANTS_PRIVATE_TEMP" TEMP="$CF_PLANTS_PRIVATE_TEMP"; exec "$@"', 'plants-list']"#.to_owned()
                };
                fs::write(&config, format!(
                    "experimental = [\"setup-scripts\", \"wrapper-scripts\"]\n{original}\n[scripts.setup.plants-private]\n{setup}\n\n[scripts.wrapper.plants-list]\n{list}\ntarget-runner = \"within-wrapper\"\n\n[[profile.plants.scripts]]\nfilter = \"all()\"\nsetup = \"plants-private\"\nlist-wrapper = \"plants-list\"\n"
                )).map_err(Error::files("write nextest's test setup in", &config))?;
                invocation = invocation.var(TEST_TEMP, temp);
            } else if matches!(reporter, Reporter::Node | Reporter::Playwright) {
                for name in ["TMPDIR", "TMP", "TEMP"] {
                    invocation = invocation.var(name, temp);
                }
            }
        }
        let invocation = match built {
            Some(folder) => invocation.var(CF_E2E_BUILT, folder),
            None => invocation,
        };
        let run = self.start(&invocation)?;
        // The environment, with the variables the invocation sets or takes away.
        let given = |name: &OsStr| invocation.vars.iter().any(|(set, _)| set == name);
        let env = Env::from_vars(
            self.checkout
                .env
                .iter()
                .filter(|(name, _)| !given(name))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .chain(
                    invocation
                        .vars
                        .iter()
                        .filter_map(|(name, value)| Some((name.clone(), value.clone()?))),
                ),
        );
        let Self {
            runtime,
            limits,
            stops,
            ..
        } = self;
        let cwd = invocation.cwd.as_path();
        let result = runtime.block_on(async {
            let mut running = pin!(capture_whole(&run, Some(cwd), &env, *limits, |_| {}));
            // Dropped with the block, the command in hand is ended whole.
            poll_fn(|cx| {
                if let Poll::Ready(done) = running.as_mut().poll(cx) {
                    return Poll::Ready(Some(done));
                }
                stops.poll(cx).map(|()| None)
            })
            .await
        });
        let execution = match result {
            None => Ok(Execution::Stopped),
            Some(done) => Ran::of(command, done).map(|mut ran| {
                ran.report = reporter.results(dir);
                Execution::Ended(ran)
            }),
        };
        drop(folder);
        execution
    }

    fn stopped(&mut self) -> bool {
        let Self { runtime, stops, .. } = self;
        runtime.block_on(async {
            // A stop is told to who listens when the runtime has its turn.
            tokio::task::yield_now().await;
            poll_fn(|cx| Poll::Ready(stops.poll(cx).is_ready())).await
        })
    }

    /// The commit the checkout is at, what git says of it beyond that (every file
    /// changed, added or untracked, ignored ones aside), and the bytes of each such
    /// file, hashed: a file put back as it was is the state it was. Where git says
    /// nothing this once (its index locked, say: no plant is tried in a checkout it says
    /// nothing of, `gitless`), a state of its own, told apart from every other: a state
    /// it cannot tell is never taken for one a command was known to pass in. A stop
    /// heard while git is asked is none.
    fn state(&mut self) -> Option<String> {
        let head = self.git(&["rev-parse", "HEAD"])?;
        let status = self.git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let (Some(head), Some(status)) = (head, status) else {
            self.untold += 1;
            return Some(format!("untold {}", self.untold));
        };
        let mut hasher = DefaultHasher::new();
        head.hash(&mut hasher);
        status.hash(&mut hasher);
        for path in changed(&status) {
            std::fs::read(self.checkout.root.join(path))
                .ok()
                .hash(&mut hasher);
        }
        Some(format!("{:016x}", hasher.finish()))
    }

    /// The checkout's root where git says nothing of it: no commit there to tell a state
    /// from (`state`). A stop heard while it is asked is the driver's to hear, which
    /// says nothing here.
    fn gitless(&mut self) -> Option<PathBuf> {
        match self.git(&["rev-parse", "HEAD"]) {
            Some(None) => Some(self.checkout.root.clone()),
            Some(Some(_)) | None => None,
        }
    }
}

/// The paths of `git status --porcelain=v1 -z`: each entry `XY <path>`, a rename's or
/// a copy's followed by the path it was.
fn changed(status: &str) -> Vec<&str> {
    let mut paths = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        paths.push(path);
        if code.contains(['R', 'C']) {
            entries.next();
        }
    }
    paths
}
