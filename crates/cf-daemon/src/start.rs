//! The daemon's start (`startDaemon`, `src/core/daemon.js:42-185`), in Node's
//! order, and what it returns: a daemon that is running, and says when it has
//! stopped.
//!
//! The order matters, and is held: the home and the files with their start
//! line; the ledger, whose lock refuses a second daemon on the same home; the
//! launch folders swept and the projects that were open suspended for a
//! resume, only once this daemon holds the ledger; the agents file folded into
//! its shape and the members' tiers read again, a failure written down and no
//! more; the UI token and the HTTP front; the stop armed; then the handle line,
//! the first thing on the standard output, the app reads it to find the daemon;
//! the bridge, the host, the engine, the pass loop and its throttles and the
//! page's operations, every handler on before the bridge is first polled and
//! the pass timer armed before the resume begins; and last the resume of what
//! was open, and a kick.

use std::cell::{OnceCell, RefCell};
use std::io::{self, Write};
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::file::{make_folder, Mkdir};
use cf_base::home::config_root;
use cf_bridge::local::{BridgeBuilder, Ended};
use cf_catalog::{roster_path, Catalog, CatalogError};
use cf_engine::seams::{Limits, Seams};
use cf_engine::Dispatcher;
use cf_harness::records::Thread;
use cf_harness::seams::{
    Bundle, LoopbackPorts, Probes, Services, SystemEntropy, SystemLoopback, SystemProcesses,
    SystemTime,
};
use cf_ledger::{open_ledger, LedgerError, Options as LedgerOptions};
use cf_proto::bridge::Role;
use cf_proto::page::HandleLine;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::watch;

use crate::api;
use crate::api::context::Context;
use crate::api::credentials::{random_hex, Credentials};
use crate::console::Console;
use crate::errors::{contain, Errors};
use crate::files::{Log, Trace};
use crate::host::{watch_exits, BridgeHost};
use crate::machine;
use crate::page::{self, Page};
use crate::pass::{throttle, PassLoop};
use crate::roster::Agents;
use crate::screens::Screens;
use crate::seams::{
    DaemonSpawn, HarnessAdapters, LaunchFolders, RandomLaunchIds, RoleTexts, WindowEnv,
};
use crate::stop::{Latch, Stopping};

/// This build's version, which the start line says in the place Node said its
/// runtime's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How soon after a change the page is told the board moved: the first change
/// begins the wait, and the rest are the same telling.
const STATE_EVENT: Duration = Duration::from_millis(100);

/// What says the handle line.
pub type OnOut = Box<dyn Fn(&HandleLine) -> io::Result<()>>;

/// What the daemon runs on, and where it says what it says.
pub struct Options {
    /// What the bridge reads: the daemon's standard input.
    pub input: Box<dyn AsyncRead + Unpin>,
    /// What the bridge writes, nothing but frames: the standard output.
    pub output: Box<dyn AsyncWrite + Unpin>,
    /// The error output, for what Node said with `console.error`.
    pub stderr: Box<dyn Write>,
    /// Says the handle line before anything else is written to the output:
    /// the app reads it as JSON, a person gets the address in words. A reader
    /// that has gone from it stops the daemon.
    pub on_out: OnOut,
    /// How the process ends, given its code: the daemon's own exit.
    pub exit: Rc<dyn Fn(i32)>,
    /// What the daemon ships beside itself.
    pub bundle: Bundle,
    /// Whether SIGTERM and SIGINT (on Windows Ctrl-C) stop the daemon.
    pub signals: bool,
}

impl Options {
    /// The daemon of this process: its standard streams, its own exit, the
    /// bundle beside its binary, and the signals that end it.
    pub fn process(on_out: OnOut) -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        Ok(Self {
            input: Box::new(tokio::io::stdin()),
            output: Box::new(tokio::io::stdout()),
            stderr: Box::new(io::stderr()),
            on_out,
            exit: Rc::new(|code| std::process::exit(code)),
            bundle: machine::bundle_of(&exe),
            signals: true,
        })
    }
}

/// Why a daemon did not start: what it says, as the CLI prints it.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("ConsensFlow has no folder to keep its things in: set CONSENSFLOW_HOME, or HOME")]
    NoHome,
    /// The home could not be made, a launch folder not swept, the output not
    /// written, a thread not started: the system's words.
    #[error("{0}")]
    System(String),
    /// The ledger's own refusals (`another ConsensFlow has <file> open`) and
    /// failures.
    #[error("{0}")]
    Ledger(#[from] LedgerError),
    #[error("{0}")]
    Catalog(#[from] CatalogError),
}

/// A daemon that is running.
pub struct Daemon {
    latch: Rc<Latch>,
    handle: HandleLine,
    stopped: watch::Receiver<bool>,
    #[cfg(test)]
    pub(crate) parts: Parts,
}

/// What a test reaches into.
#[cfg(test)]
pub(crate) struct Parts {
    pub(crate) ledger: Rc<RefCell<cf_ledger::Ledger>>,
    pub(crate) engine: Rc<dyn crate::page::Engine>,
    pub(crate) home: std::path::PathBuf,
}

impl Daemon {
    /// What the handle line said.
    pub fn handle(&self) -> &HandleLine {
        &self.handle
    }

    /// Asks it to stop, for `why` (`stop(why)`); one that is stopping already
    /// is not asked again.
    pub fn stop(&self, why: &str) {
        self.latch.trip(why);
    }

    /// Ends once the stop has run to its end: for a daemon whose `exit` returns
    /// (a test's). The process's own never returns from its exit.
    pub async fn finished(&self) {
        let mut stopped = self.stopped.clone();
        let _ = stopped.wait_for(|stopped| *stopped).await;
    }
}

/// Starts a daemon on the local set the caller is in, over the home `env`
/// names.
pub async fn start(env: Env, options: Options) -> Result<Daemon, StartError> {
    // The home, and the files in it: every event and every change of a window
    // goes to events.jsonl as it happens, and what is worth knowing afterwards
    // to daemon.log, the start first.
    let home = config_root(&env).ok_or(StartError::NoHome)?;
    make_folder(&home, 0o777, Mkdir::Sync)
        .map_err(|failed| StartError::System(failed.to_string()))?;
    let trace = Rc::new(Trace::new(&home));
    let log = Rc::new(Log::new(&home));
    let errors = Rc::new(Errors::new(Rc::clone(&log), Rc::clone(&trace)));
    log.info(&format!(
        "start pid {} rust {VERSION} home {}",
        std::process::id(),
        home.display()
    ));

    // The ledger, whose lock refuses a second daemon: nothing of the running
    // one is touched before it is held.
    let told = Rc::clone(&trace);
    let ledger = open_ledger(
        &home.join("consensflow.db"),
        LedgerOptions {
            trace: Box::new(move |event| told.event(event)),
            ..LedgerOptions::default()
        },
    )?;
    let ledger = Rc::new(RefCell::new(ledger));
    // No window survives a restart: what every launch left in the home goes.
    let swept = cf_harness::sweep_launches(&home.to_string_lossy())
        .map_err(|failed| StartError::System(failed.to_string()))?;
    if swept > 0 {
        log.info(&format!(
            "swept {swept} launch folder{}",
            if swept == 1 { "" } else { "s" }
        ));
    }
    ledger.borrow_mut().suspend_for_restart()?;
    // What the app ships is what the roster has, with the human's own agents,
    // and every member's tier is read again from its agent. An agents file that
    // cannot be read or rewritten stops no start.
    let agents = Rc::new(Agents::new(
        Catalog::bundled()?,
        roster_path(&env).ok_or(StartError::NoHome)?,
    ));
    if let Err(cause) = agents.normalize_and_follow(&mut ledger.borrow_mut()) {
        log.error("the agents file could not be used", Some(&cause));
    }

    // The stop's latch, and the bridge as far as building it goes: it hears
    // and says nothing until it is polled, after the handle line.
    let latch = Latch::new();
    let console = Rc::new(Console::to(options.stderr, {
        let latch = Rc::clone(&latch);
        // The drain of `cf ui` (`bin/cf.mjs:15-28`): no reader, no point.
        move || latch.trip("asked to stop")
    }));
    let (bridge, connection) = BridgeBuilder::new(Role::Daemon)
        .on_fatal({
            let (log, latch) = (Rc::clone(&log), Rc::clone(&latch));
            move |cause| {
                log.error("the bridge failed", Some(&cause.to_string()));
                latch.trip("the bridge failed");
            }
        })
        .connect(options.input, options.output);

    // The tokens, and the HTTP front the agents' and the human's requests come
    // to. The pass loop does not exist yet: a kick waits for it.
    let credentials = Rc::new(Credentials::new());
    let token = random_hex(24);
    let passes: Rc<OnceCell<PassLoop>> = Rc::new(OnceCell::new());
    let kick: Rc<dyn Fn()> = {
        let passes = Rc::clone(&passes);
        Rc::new(move || {
            if let Some(passes) = passes.get() {
                passes.kick();
            }
        })
    };
    let context = Rc::new(Context {
        ledger: Rc::clone(&ledger),
        credentials: Rc::clone(&credentials),
        kick: Rc::clone(&kick),
        closing: crate::api::context::Closing::new(),
        roster: Rc::clone(&agents) as Rc<dyn crate::api::context::AgentRows>,
        log: Rc::clone(&log),
        trace: Rc::clone(&trace),
    });
    let screens = Rc::new(Screens {
        token: token.clone(),
        on_roster_change: {
            let (agents, ledger, bridge, kick) = (
                Rc::clone(&agents),
                Rc::clone(&ledger),
                bridge.clone(),
                Rc::clone(&kick),
            );
            Rc::new(move || {
                let moved = agents.follow(&mut ledger.borrow_mut())?;
                if !moved.is_empty() {
                    bridge.event("state.changed", json!({ "reason": "roster" }));
                }
                kick();
                Ok(())
            })
        },
    });
    let api = Rc::new(
        api::serve(Rc::clone(&context), screens, Rc::clone(&errors))
            .await
            .map_err(|failed| StartError::System(failed.to_string()))?,
    );

    // What the engine runs windows with.
    let processes = Rc::new(SystemProcesses::new(env.clone()));
    let time = Rc::new(SystemTime);
    let zone = machine::zone();
    let records = Rc::new(
        Thread::new(env.clone(), zone.clone(), Rc::clone(&time) as _)
            .map_err(|failed| StartError::System(failed.to_string()))?,
    );
    let services = Services {
        env: env.clone(),
        records: Rc::clone(&records) as _,
        time: Rc::clone(&time) as _,
        entropy: Rc::new(SystemEntropy),
        ports: Rc::new(LoopbackPorts),
        loopback: Rc::new(SystemLoopback),
        processes: Rc::clone(&processes) as _,
        probes: Rc::new(Probes::default()),
        bundle: options.bundle.clone(),
        zone,
    };
    let seams = Seams {
        ledger: Rc::clone(&ledger),
        host: Rc::new(BridgeHost::new(bridge.clone(), env.clone())),
        adapters: Rc::new(HarnessAdapters::new(&services)),
        records,
        time,
        launch_ids: Rc::new(RandomLaunchIds),
        credentials: Rc::clone(&credentials) as _,
        pane_env: Rc::new(WindowEnv::new(
            &env,
            api.url(),
            &options.bundle.bin.to_string_lossy(),
        )),
        roster: Rc::clone(&agents) as _,
        roles: Rc::new(RoleTexts::new(env.clone(), options.bundle.pane_cf.clone())),
        trace: Rc::clone(&trace) as _,
        log: Rc::clone(&log) as _,
        launch_files: Rc::new(LaunchFolders::new(home.clone(), Rc::clone(&errors))),
        spawn: Rc::new(DaemonSpawn::new(Rc::clone(&errors))),
        limits: Limits::default(),
    };
    let dispatcher = Dispatcher::new(seams);

    // The stop is armed before the handle line: whatever the app does once it
    // has read it finds a daemon that can stop. Every trigger trips the one
    // latch; the stop itself runs once, whichever comes first.
    let (stopped_tx, stopped) = watch::channel(false);
    let stopping = Rc::new(Stopping {
        errors: Rc::clone(&errors),
        passes: Rc::clone(&passes),
        api: Rc::clone(&api),
        ends_children: {
            let processes = Rc::clone(&processes);
            Rc::new(move || processes.end_all())
        },
        ledger: Rc::clone(&ledger),
        exit: Rc::clone(&options.exit),
    });
    arm(
        &latch,
        bridge.ended(),
        options.signals,
        &errors,
        stopping,
        stopped_tx,
    );

    // The handle line says the daemon is ready.
    let handle = HandleLine {
        url: format!("{}/", api.url()),
        token,
    };
    if let Err(failed) = (options.on_out)(&handle) {
        if failed.kind() == io::ErrorKind::BrokenPipe {
            latch.trip("asked to stop");
        } else {
            return Err(StartError::System(failed.to_string()));
        }
    }

    // The engine's pass on a timer, armed before the resume begins, and what
    // the page is told as the board moves, a hundred milliseconds after the
    // first change and not put off by the rest.
    let pass_dispatcher = Rc::clone(&dispatcher);
    let passes_running = PassLoop::start(
        Box::new(move || {
            let dispatcher = Rc::clone(&pass_dispatcher);
            Box::pin(async move { dispatcher.pass().await.map_err(|failed| failed.to_string()) })
        }),
        Rc::clone(&errors),
        Rc::clone(&console),
    );
    // Set once: nothing between the stop's arming and here awaits.
    let _ = passes.set(passes_running);
    {
        let bridge = bridge.clone();
        dispatcher.on_change(throttle(STATE_EVENT, move || {
            bridge.event("state.changed", json!({ "reason": "core" }));
        }));
    }
    {
        // A window that wrote more changes only a view of what it did: the
        // page reads that again, not the board.
        let bridge = bridge.clone();
        dispatcher.on_transcript(throttle(STATE_EVENT, move || {
            bridge.event("state.changed", json!({ "reason": "transcript" }));
        }));
    }

    // The page's operations and the host's exits, every handler on before the
    // bridge is first polled.
    let page = Rc::new(Page {
        ledger: Rc::clone(&ledger),
        engine: Rc::new(Rc::clone(&dispatcher)),
        env: env.clone(),
        kick: Rc::clone(&kick),
    });
    page::register(&bridge, &page, &errors);
    let exiting = Rc::clone(&dispatcher);
    watch_exits(&bridge, Rc::clone(&errors), move |pane| {
        exiting.pane_exited(pane)
    });
    errors.spawn("the bridge's task failed", connection);

    // What was open comes back, and the loop is woken either way.
    let (resuming, resumed_console) = (Rc::clone(&dispatcher), Rc::clone(&console));
    let (resume_errors, resume_kick) = (Rc::clone(&errors), Rc::clone(&kick));
    errors.spawn("the resume failed", async move {
        match contain(resuming.resume_after_restart()).await {
            Ok(Ok(outcomes)) => {
                for outcome in outcomes {
                    if let Some(cause) = outcome.error {
                        resumed_console
                            .line(&format!("consensflow resume {}: {cause}", outcome.project));
                        resume_errors.log().error(
                            &format!("resume of project {} failed", outcome.project),
                            Some(&cause),
                        );
                    }
                }
            }
            Ok(Err(failed)) => {
                resume_errors
                    .log()
                    .error("the resume failed", Some(&failed.to_string()));
            }
            Err(panicked) => resume_errors.caught("the resume failed", &panicked),
        }
        resume_kick();
    });

    Ok(Daemon {
        latch,
        handle,
        stopped,
        #[cfg(test)]
        parts: Parts {
            ledger,
            engine: Rc::new(dispatcher),
            home,
        },
    })
}

/// Arms the stop: every way the daemon is asked to end trips `latch`, and
/// what trips it runs `stopping`, once.
fn arm(
    latch: &Rc<Latch>,
    ended: impl std::future::Future<Output = Ended> + 'static,
    signals: bool,
    errors: &Rc<Errors>,
    stopping: Rc<Stopping>,
    done: watch::Sender<bool>,
) {
    let watching = Rc::clone(latch);
    errors.spawn("the stop's watch failed", async move {
        // Why the input ended is told by the bridge; a failure is told to the
        // fatal handler too, which has said it first.
        match ended.await {
            Ended::Input => watching.trip("stdin ended"),
            Ended::Closed => watching.trip("stdin closed"),
            Ended::Failed(_) => watching.trip("the bridge failed"),
        }
    });
    if signals {
        listen_for_signals(latch, errors);
    }
    let (waiting, stop_errors) = (Rc::clone(latch), Rc::clone(errors));
    errors.spawn("the stop failed", async move {
        let why = waiting.tripped().await;
        // The tail must be reached whatever goes wrong before it.
        if let Err(panicked) = contain(stopping.run(&why)).await {
            stop_errors.caught("the stop failed", &panicked);
            stopping.tail();
        }
        done.send_replace(true);
    });
}

/// SIGTERM and SIGINT, which stop the daemon; on Windows Ctrl-C, and the end
/// of the input, are all there is.
fn listen_for_signals(latch: &Rc<Latch>, errors: &Rc<Errors>) {
    #[cfg(unix)]
    for (kind, name) in [
        (tokio::signal::unix::SignalKind::terminate(), "SIGTERM"),
        (tokio::signal::unix::SignalKind::interrupt(), "SIGINT"),
    ] {
        if let Ok(mut signal) = tokio::signal::unix::signal(kind) {
            let latch = Rc::clone(latch);
            errors.spawn("a signal's watch failed", async move {
                signal.recv().await;
                latch.trip(name);
            });
        }
    }
    #[cfg(not(unix))]
    {
        let latch = Rc::clone(latch);
        errors.spawn("a signal's watch failed", async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                latch.trip("SIGINT");
            }
        });
    }
}

#[cfg(test)]
mod tests;
